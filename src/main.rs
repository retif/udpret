use tokio::net::UdpSocket;
use tokio::time::{timeout, Duration};

async fn udp_probe(target: &str, port: u16) -> bool {
    // Combine target and port to create a full address
    let addr = format!("{}:{}", target, port);
    
    // Create a UDP socket bound to any local port
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(sock) => sock,
        Err(_) => return false,
    };

    // Set a short timeout for the probe
    let probe_timeout = Duration::from_secs(2);

    // Dummy data to send as a probe packet
    let probe_data = b"probe";

    // Attempt to send the probe packet
    match socket.send_to(probe_data, &addr).await {
        Ok(_) => {
            // Wait for any response or timeout
            let mut buf = [0u8; 1024];
            match timeout(probe_timeout, socket.recv_from(&mut buf)).await {
                Ok(Ok((_len, _src))) => {
                    // If we receive a response, consider the port open
                    println!("UDP port {} is open on {}", port, target);
                    true
                }
                Ok(Err(e)) => {
                    // Error in receiving the response
                    eprintln!("Error receiving response: {}", e);
                    false
                }
                Err(_) => {
                    // Timed out, consider the port potentially open but unresponsive
                    println!("No response from UDP port {} on {}", port, target);
                    false
                }
            }
        }
        Err(e) => {
            // Failed to send the probe packet
            eprintln!("Failed to send probe to {}: {}: {}", target, port, e);
            false
        }
    }
}

#[tokio::main]
async fn main() {
    let target = "127.0.0.1";
    let port = 8080;

    if udp_probe(target, port).await {
        println!("Port {} is likely open on {}", port, target);
    } else {
        println!("Port {} is likely closed or unresponsive on {}", port, target);
    }
}
