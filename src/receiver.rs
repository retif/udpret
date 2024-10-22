use tokio::{net::UdpSocket, sync::{mpsc, Mutex}, time::{interval, Duration}};
use std::collections::VecDeque;
use std::net::SocketAddr;
use crossterm::{ExecutableCommand, cursor, terminal};
use std::io::{stdout, Write};
use clap::{Arg, Command};
use std::sync::Arc;

const MAX_RECENT_PACKETS: usize = 10; // Maximum number of recent packets to display
const CHANNEL_BUFFER_SIZE: usize = 100; // Buffer size for the channel

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Define command-line arguments using clap
    let matches = Command::new("receiver")
        .version("1.0")
        .author("Your Name")
        .about("Asynchronous UDP receiver with enhanced features")
        .arg(
            Arg::new("listen")
                .short('l')
                .long("listen")
                .value_name("IP:PORT")
                .help("Sets the IP and port to listen for incoming UDP packets")
                .required(true)
        )
        .arg(
            Arg::new("buffer")
                .short('b')
                .long("buffer")
                .value_name("BYTES")
                .help("Sets the receive buffer size in bytes")
                .default_value("65535")
        )
        .arg(
            Arg::new("window")
                .short('w')
                .long("window")
                .value_name("INTERVALS")
                .help("Sets the sliding window size for receive rate tracking")
                .default_value("10")
        )
        .get_matches();

    // Get command-line arguments
    let listen_addr: String = matches.get_one::<String>("listen").unwrap().clone();
    let buffer_size: usize = matches.get_one::<String>("buffer").unwrap().parse().expect("Invalid buffer size");
    let window_size: usize = matches.get_one::<String>("window").unwrap().parse().expect("Invalid window size");

    // Parse the listening address with explicit type annotation
    let listen_addr: SocketAddr = listen_addr.parse().expect("Invalid listening address");

    // Create and bind the UDP socket for receiving
    let socket = UdpSocket::bind(listen_addr).await.expect("Failed to bind listening socket");
    println!("Receiver listening on {}", listen_addr);

    // Wrap the socket in Arc<Mutex<>> for safe sharing
    let socket = Arc::new(Mutex::new(socket));

    // Create a channel to decouple packet reception from processing
    let (tx, mut rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);

    // Clone the socket for the reception task
    let socket_clone = Arc::clone(&socket);

    // Spawn a separate task for packet reception
    tokio::spawn(async move {
        let mut buf = vec![0u8; buffer_size];
        loop {
            let socket = socket_clone.lock().await;
            match socket.recv_from(&mut buf).await {
                Ok((len, _src_addr)) => {
                    if tx.send(buf[..len].to_vec()).await.is_err() {
                        eprintln!("Failed to send packet to processing task");
                    }
                }
                Err(e) => eprintln!("Error receiving packet: {}", e),
            }
        }
    });

    // Initialize sliding window tracking
    let mut packet_rates: VecDeque<f64> = VecDeque::with_capacity(window_size);
    let mut byte_rates: VecDeque<f64> = VecDeque::with_capacity(window_size);

    // Initialize statistics
    let mut total_packets = 0;
    let mut total_bytes = 0;
    let mut recent_packets: VecDeque<Vec<u8>> = VecDeque::with_capacity(MAX_RECENT_PACKETS);

    // Accumulators for current interval
    let mut interval_packets = 0;
    let mut interval_bytes = 0;

    // Set up the interval for updating the terminal display
    let update_interval = Duration::from_secs_f64(1.0 / 10.0); // 10 FPS
    let mut interval_timer = interval(update_interval);

    // Set up terminal
    let mut stdout = stdout();

    // Main processing loop
    loop {
        tokio::select! {
            // Receive packets from the channel
            Some(packet) = rx.recv() => {
                total_packets += 1;
                total_bytes += packet.len();
                interval_packets += 1;
                interval_bytes += packet.len();

                // Add to recent packets buffer
                if recent_packets.len() == MAX_RECENT_PACKETS {
                    recent_packets.pop_front();
                }
                recent_packets.push_back(packet);
            }

            // Terminal update
            _ = interval_timer.tick() => {
                // Update sliding window rates
                update_sliding_window(&mut packet_rates, interval_packets as f64, window_size);
                update_sliding_window(&mut byte_rates, interval_bytes as f64, window_size);

                // Reset interval accumulators
                interval_packets = 0;
                interval_bytes = 0;

                // Calculate sliding window averages, scaled to packets/sec
                let interval_duration_sec = 0.1; // 10 FPS
                let avg_packet_rate = if !packet_rates.is_empty() {
                    packet_rates.iter().sum::<f64>() / (packet_rates.len() as f64 * interval_duration_sec)
                } else {
                    0.0 // No packets received yet
                };

                // Calculate sliding byte rate, scaled to bytes/sec
                let avg_byte_rate = if !byte_rates.is_empty() {
                    byte_rates.iter().sum::<f64>() / (byte_rates.len() as f64 * interval_duration_sec)
                } else {
                    0.0 // No bytes received yet
                };

                // Clear the terminal and move the cursor to the top-left corner
                if let Err(e) = stdout.execute(terminal::Clear(terminal::ClearType::All)) {
                    eprintln!("Failed to clear terminal: {}", e);
                }
                if let Err(e) = stdout.execute(cursor::MoveTo(0, 0)) {
                    eprintln!("Failed to move cursor: {}", e);
                }

                // Print the statistics
                if let Err(e) = writeln!(
                    stdout,
                    "Receiver Statistics:\n\
                    ----------------\n\
                    Total Packets Received: {}\n\
                    Total Bytes Received: {}\n\
                    Sliding Packet Rate: {:>8.2} packets/sec\n\
                    Sliding Byte Rate: {:>8.2} bytes/sec\n\
                    \nLast {} Packets:\n",
                    total_packets, total_bytes, avg_packet_rate, avg_byte_rate, MAX_RECENT_PACKETS
                ) {
                    eprintln!("Failed to write to terminal: {}", e);
                }

                // Display the last 10 packets received
                for packet in recent_packets.iter() {
                    let content = format!("{:?}", packet);
                    let max_data_width = (80 as usize).saturating_sub(30);
                    let truncated_content = if content.len() > max_data_width {
                        format!("{}...", &content[..max_data_width.saturating_sub(3)])
                    } else {
                        content
                    };
                    if let Err(e) = writeln!(stdout, "{}", truncated_content) {
                        eprintln!("Failed to write packet content: {}", e);
                    }
                }

                // Flush the terminal output
                if let Err(e) = stdout.flush() {
                    eprintln!("Failed to flush terminal: {}", e);
                }
            }
        }
    }
}

// Updates the sliding window with a new rate value
fn update_sliding_window(window: &mut VecDeque<f64>, value: f64, window_size: usize) {
    if window.len() == window_size {
        window.pop_front();
    }
    window.push_back(value);
}
