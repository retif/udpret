use tokio::{net::UdpSocket, time::{sleep, Duration, interval}};
use std::collections::VecDeque;
use crossterm::{ExecutableCommand, cursor, terminal};
use std::io::{stdout, Write, Error as IoError}; // Import IoError
use std::net::SocketAddr; // Import SocketAddr
use clap::{Arg, Command};
use rand::Rng;

const DEFAULT_RETRY_COUNT: usize = 3;
const DEFAULT_RETRY_DELAY_MS: u64 = 100;
const MAX_RECENT_PACKETS: usize = 10;
const DEFAULT_WINDOW_SIZE: usize = 10; // Default sliding window size

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Define command-line arguments using clap
    let matches = Command::new("sender")
        .version("1.0")
        .author("Your Name")
        .about("Asynchronous UDP sender with enhanced features")
        .arg(
            Arg::new("destination")
                .short('d')
                .long("destination")
                .value_name("IP:PORT")
                .help("Sets the IP and port to send UDP packets")
                .required(true)
        )
        .arg(
            Arg::new("rate")
                .short('r')
                .long("rate")
                .value_name("PACKETS/SEC")
                .help("Sets the target send rate in packets per second")
                .default_value("10")
        )
        .arg(
            Arg::new("size")
                .short('s')
                .long("size")
                .value_name("BYTES")
                .help("Sets the packet size in bytes")
                .default_value("256")
        )
        .arg(
            Arg::new("retries")
                .short('t')
                .long("retries")
                .value_name("COUNT")
                .help("Sets the number of retries for sending packets")
                .default_value("3")
        )
        .arg(
            Arg::new("retry_delay")
                .long("retry-delay")
                .value_name("MS")
                .help("Sets the delay between retries in milliseconds")
                .default_value("100")
        )
        .arg(
            Arg::new("window")
                .short('w')
                .long("window")
                .value_name("INTERVALS")
                .help("Sets the sliding window size for send rate tracking")
                .default_value("10")
        )
        .get_matches();

    // Get command-line arguments
    let destination: String = matches.get_one::<String>("destination").unwrap().clone();
    let target_rate: u32 = matches.get_one::<String>("rate").unwrap().parse().expect("Invalid rate");
    let packet_size: usize = matches.get_one::<String>("size").unwrap().parse().expect("Invalid packet size");
    let max_retries: usize = matches.get_one::<String>("retries").unwrap().parse().expect("Invalid retry count");
    let retry_delay_ms: u64 = matches.get_one::<String>("retry_delay").unwrap().parse().expect("Invalid retry delay");
    let window_size: usize = matches.get_one::<String>("window").unwrap().parse().expect("Invalid window size");

    // Parse the destination address
    let dest_addr: SocketAddr = destination.parse().expect("Invalid destination address");

    // Create the UDP socket
    let socket = UdpSocket::bind("0.0.0.0:0").await.expect("Failed to bind UDP socket");

    // Initialize sliding window tracking
    let mut packet_rates: VecDeque<f64> = VecDeque::with_capacity(window_size);
    let mut byte_rates: VecDeque<f64> = VecDeque::with_capacity(window_size);

    // Initialize statistics
    let mut total_packets = 0;
    let mut total_bytes = 0;

    // Set up the interval for controlling the send rate
    let send_interval = Duration::from_secs_f64(1.0 / target_rate as f64);
    let mut interval_timer = interval(send_interval);

    // Set up terminal
    let mut stdout = stdout();
    if let Err(e) = stdout.execute(terminal::Clear(terminal::ClearType::All)) {
        eprintln!("Failed to clear terminal: {}", e);
    }

    loop {
        interval_timer.tick().await;

        // Generate random payload
        let payload = generate_payload(packet_size);

        // Attempt to send the packet with retries
        match send_with_retries(&socket, &payload, dest_addr, max_retries, retry_delay_ms).await {
            Ok(bytes_sent) => {
                total_packets += 1;
                total_bytes += bytes_sent;
                update_sliding_window(&mut packet_rates, target_rate as f64, window_size);
                update_sliding_window(&mut byte_rates, bytes_sent as f64, window_size);
            }
            Err(e) => {
                eprintln!("Failed to send packet after retries: {}", e);
            }
        }

        // Calculate sliding window averages
        let avg_packet_rate = packet_rates.iter().sum::<f64>() / packet_rates.len() as f64;
        let avg_byte_rate = byte_rates.iter().sum::<f64>() / byte_rates.len() as f64;

        // Fetch terminal size and adjust display
        let (cols, _) = terminal::size().unwrap_or((80, 20));
        let max_data_width = (cols as usize).saturating_sub(30);

        // Clear and redraw terminal
        if let Err(e) = stdout.execute(cursor::MoveTo(0, 0)) {
            eprintln!("Failed to move cursor: {}", e);
        }
        if let Err(e) = writeln!(
            stdout,
            "Sender Statistics:\n\
            ----------------\n\
            Total Packets Sent: {}\n\
            Total Bytes Sent: {}\n\
            Sliding Packet Rate: {:>8.2} packets/sec\n\
            Sliding Byte Rate: {:>8.2} bytes/sec\n\
            \nCurrent Payload (truncated):\n\
            {}\n",
            total_packets, total_bytes, avg_packet_rate, avg_byte_rate, format!("{:?}", &payload[..max_data_width.min(payload.len())])
        ) {
            eprintln!("Failed to write to terminal: {}", e);
        }

        if let Err(e) = stdout.flush() {
            eprintln!("Failed to flush terminal: {}", e);
        }
    }
}

// Generates a random payload of specified size
fn generate_payload(size: usize) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    (0..size).map(|_| rng.gen()).collect()
}

// Attempts to send a packet with retries
async fn send_with_retries(
    socket: &UdpSocket,
    data: &[u8],
    addr: SocketAddr,
    max_retries: usize,
    retry_delay_ms: u64
) -> Result<usize, IoError> {
    let mut retries = 0;
    loop {
        match socket.send_to(data, addr).await {
            Ok(bytes_sent) => return Ok(bytes_sent),
            Err(e) if retries < max_retries => {
                retries += 1;
                eprintln!("Error sending packet to {}: {}. Retrying {}/{}...", addr, e, retries, max_retries);
                sleep(Duration::from_millis(retry_delay_ms)).await;
            }
            Err(e) => return Err(e),
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
