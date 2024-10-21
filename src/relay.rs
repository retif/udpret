use tokio::{net::UdpSocket, sync::{mpsc, Mutex}, time::{interval, Duration}};
use std::collections::VecDeque;
use crossterm::{ExecutableCommand, cursor, terminal};
use std::io::{stdout, Write, Error as IoError};
use std::net::SocketAddr;
use clap::{Arg, Command};
use std::sync::Arc;

const MAX_RECENT_PACKETS: usize = 10;
const ROLLING_WINDOW: usize = 300; // 300 intervals for sliding window

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Define command-line arguments using clap
    let matches = Command::new("relay")
        .version("1.0")
        .author("Your Name")
        .about("Asynchronous UDP relay with adjustable frame rate")
        .arg(
            Arg::new("listen")
                .short('l')
                .long("listen")
                .value_name("IP:PORT")
                .help("Sets the IP and port to listen for incoming UDP packets")
                .required(true)
        )
        .arg(
            Arg::new("forward")
                .short('f')
                .long("forward")
                .value_name("IP:PORT")
                .help("Sets the IP and port to forward the UDP packets")
                .required(true)
        )
        .arg(
            Arg::new("framerate")
                .short('r')
                .long("framerate")
                .value_name("FPS")
                .help("Sets the terminal redraw rate (frames per second)")
                .default_value("30")
        )
        .get_matches();

    // Get the listening and forwarding addresses
    let listen_addr: SocketAddr = matches.get_one::<String>("listen").unwrap()
        .parse().expect("Invalid listening address");
    let forward_addr: SocketAddr = matches.get_one::<String>("forward").unwrap()
        .parse().expect("Invalid forwarding address");

    // Get the frame rate and calculate the update interval
    let framerate: u32 = matches.get_one::<String>("framerate").unwrap()
        .parse().expect("Invalid frame rate");
    let update_interval = Duration::from_secs_f64(1.0 / framerate as f64);

    // Create and bind the UDP socket for listening
    let socket = UdpSocket::bind(listen_addr).await?;
    println!("Relay server listening on {}", listen_addr);

    // Create channel for forwarding statistics
    let (stat_tx, stat_rx) = mpsc::channel(100);

    // Shared state for destination reachability
    let destination_open = Arc::new(Mutex::new(true));

    // Start packet reception task
    tokio::spawn(receive_and_forward_packets(socket, forward_addr, stat_tx.clone()));

    // Start destination probing task
    let probe_addr = forward_addr;
    let destination_open_clone = Arc::clone(&destination_open);
    tokio::spawn(async move {
        probe_destination(probe_addr, destination_open_clone).await;
    });

    // Start statistics display task
    display_statistics(stat_rx, update_interval, framerate, destination_open).await;

    Ok(())
}

// Task to receive packets and forward them, with detailed error handling
async fn receive_and_forward_packets(
    socket: UdpSocket,
    forward_addr: SocketAddr,
    stat_tx: mpsc::Sender<(Vec<u8>, SocketAddr, bool)>
) {
    let mut buf = vec![0u8; 65535];
    let forward_socket = UdpSocket::bind("0.0.0.0:0").await.expect("Failed to bind forwarding socket");

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, src_addr)) => {
                // Attempt to forward the packet and track if it succeeds
                let success = forward_packet(&forward_socket, &buf[..len], forward_addr).await.is_ok();

                // Send packet data and success status to statistics channel
                if let Err(e) = stat_tx.send((buf[..len].to_vec(), src_addr, success)).await {
                    eprintln!("Failed to send statistics to channel: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Error receiving packet: {}", e);
                continue;
            }
        }
    }
}

// Helper function to forward packets without retries
async fn forward_packet(socket: &UdpSocket, data: &[u8], addr: SocketAddr) -> Result<(), IoError> {
    match socket.send_to(data, addr).await {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Failed to send packet to {}: {}", addr, e);
            Err(e)
        }
    }
}

// Task to periodically probe the destination UDP port
async fn probe_destination(forward_addr: SocketAddr, destination_open: Arc<Mutex<bool>>) {
    let probe_socket = UdpSocket::bind("0.0.0.0:0").await.expect("Failed to bind probing socket");
    let probe_data = b"probe";
    let probe_interval = Duration::from_secs(5);

    loop {
        tokio::time::sleep(probe_interval).await;

        // Attempt to send a probe packet
        let send_result = probe_socket.send_to(probe_data, forward_addr).await;
        let mut is_open = true;

        match send_result {
            Ok(_) => {
                // Wait for a response or timeout
                let mut buf = [0u8; 1024];
                match tokio::time::timeout(probe_interval, probe_socket.recv_from(&mut buf)).await {
                    Ok(Ok(_)) => is_open = true, // Received a response, port is open
                    Ok(Err(_)) => is_open = false, // OS error, consider the port closed
                    Err(_) => is_open = true, // No response, assume open but unresponsive
                }
            }
            Err(_) => {
                is_open = false; // OS error, consider the port closed
            }
        }

        // Update shared state
        let mut dest_open = destination_open.lock().await;
        *dest_open = is_open;
    }
}

// Task to display statistics and the last 10 packets with a sliding window average
async fn display_statistics(
    mut stat_rx: mpsc::Receiver<(Vec<u8>, SocketAddr, bool)>, 
    update_interval: Duration, 
    framerate: u32,
    destination_open: Arc<Mutex<bool>>
) {
    let mut total_packets = 0;
    let mut total_bytes = 0;
    let mut packets_in_last_interval = 0;
    let mut bytes_in_last_interval = 0;
    let mut recent_packets: VecDeque<(Vec<u8>, SocketAddr, bool)> = VecDeque::with_capacity(MAX_RECENT_PACKETS);
    let mut failed_sends = 0; // Track failed sends

    // Add explicit type annotations for VecDeque
    let mut packet_rates: VecDeque<f64> = VecDeque::with_capacity(ROLLING_WINDOW);
    let mut byte_rates: VecDeque<f64> = VecDeque::with_capacity(ROLLING_WINDOW);

    // Set up the interval timer based on the frame rate
    let mut interval_timer = interval(update_interval);

    // Set up terminal
    let mut stdout = stdout();
    if let Err(e) = stdout.execute(terminal::Clear(terminal::ClearType::All)) {
        eprintln!("Failed to clear terminal: {}", e);
    }

    loop {
        // Wait for the next interval tick
        interval_timer.tick().await;

        // Track the number of pending messages in the receive buffer
        let receive_buffer_size = stat_rx.len();

        // Collect stats from the receiver
        while let Ok((data, src_addr, success)) = stat_rx.try_recv() {
            total_packets += 1;
            total_bytes += data.len();
            packets_in_last_interval += 1;
            bytes_in_last_interval += data.len();

            // Update success or failure tracking
            if !success {
                failed_sends += 1;
            }

            // Store recent packet info
            if recent_packets.len() == MAX_RECENT_PACKETS {
                recent_packets.pop_front();
            }
            recent_packets.push_back((data, src_addr, success));
        }

        // Calculate raw rates for the last interval, scaling to packets/sec
        let raw_packet_rate = packets_in_last_interval as f64 * framerate as f64;
        let raw_byte_rate = bytes_in_last_interval as f64 * framerate as f64;

        // Update sliding window averages
        if packet_rates.len() == ROLLING_WINDOW {
            packet_rates.pop_front();
            byte_rates.pop_front();
        }
        packet_rates.push_back(raw_packet_rate);
        byte_rates.push_back(raw_byte_rate);

        // Calculate the sliding window average
        let avg_packet_rate = packet_rates.iter().sum::<f64>() / packet_rates.len() as f64;
        let avg_byte_rate = byte_rates.iter().sum::<f64>() / byte_rates.len() as f64;

        // Reset interval counters
        packets_in_last_interval = 0;
        bytes_in_last_interval = 0;

        // Format the total bytes and sliding byte rate
        let formatted_total_bytes = format_bytes(total_bytes as f64);
        let formatted_byte_rate = format_bytes(avg_byte_rate);

        // Fetch terminal size and adjust display
        let (cols, _) = terminal::size().unwrap_or((80, 20));
        let max_data_width = (cols as usize).saturating_sub(30);

        // Clear and redraw terminal
        if let Err(e) = stdout.execute(cursor::MoveTo(0, 0)) {
            eprintln!("Failed to move cursor: {}", e);
        }

        // Check destination reachability status
        let dest_open = *destination_open.lock().await;
        let connectivity_status = if dest_open {
            "Destination Reachable"
        } else {
            "Destination Unreachable"
        };

        if let Err(e) = writeln!(
            stdout,
            "Relay Statistics:\n\
            ----------------\n\
            Total Packets Forwarded: {}\n\
            Total Bytes Forwarded: {} {}\n\
            Sliding Packet Rate: {:>8.2} packets/sec\n\
            Sliding Byte Rate: {:>8} {}/sec\n\
            Receive Buffer Size: {:>8}\n\
            Failed Sends: {}\n\
            Connectivity: {}\n\
            \nLast {} Packets:\n",
            format_number(total_packets),
            formatted_total_bytes.0, formatted_total_bytes.1,
            avg_packet_rate,
            formatted_byte_rate.0, formatted_byte_rate.1,
            receive_buffer_size,
            failed_sends,
            connectivity_status,
            MAX_RECENT_PACKETS
        ) {
            eprintln!("Failed to write to terminal: {}", e);
        }

        // Display last 10 packets with truncated content
        for (data, addr, success) in recent_packets.iter() {
            let content = format!("{:?}", data);
            let truncated_content = if content.len() > max_data_width {
                format!("{}...", &content[..max_data_width.saturating_sub(3)])
            } else {
                content
            };
            if let Err(e) = writeln!(
                stdout, 
                "From {}: {} [{}]", 
                addr, 
                truncated_content, 
                if *success { "Sent" } else { "Failed" }
            ) {
                eprintln!("Failed to write packet content: {}", e);
            }
        }

        if let Err(e) = stdout.flush() {
            eprintln!("Failed to flush terminal: {}", e);
        }
    }
}

// Formats a large number with comma separators
fn format_number(n: u64) -> String {
    let mut s = n.to_string();
    let mut result = String::new();

    while s.len() > 3 {
        let len = s.len();
        result = format!(",{}{}", &s[len - 3..], result);
        s.truncate(len - 3);
    }
    result = format!("{}{}", s, result);

    result
}

// Converts bytes to a human-readable format and returns the value and unit
fn format_bytes(bytes: f64) -> (String, &'static str) {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    if bytes >= TB {
        (format!("{:.2}", bytes / TB), "TB")
    } else if bytes >= GB {
        (format!("{:.2}", bytes / GB), "GB")
    } else if bytes >= MB {
        (format!("{:.2}", bytes / MB), "MB")
    } else if bytes >= KB {
        (format!("{:.2}", bytes / KB), "KB")
    } else {
        (format!("{:.2}", bytes), "B")
    }
}
