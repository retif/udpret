## UDP Retransmitter

```bash
$ ./relay --help
Asynchronous UDP relay

Usage: relay [OPTIONS] --listen <IP:PORT> --forward <IP:PORT>

Options:
  -l, --listen <IP:PORT>   Sets the IP and port to listen for incoming UDP packets
  -f, --forward <IP:PORT>  Sets the IP and port to forward the UDP packets
  -r, --framerate <FPS>    Sets the terminal redraw rate (frames per second) [default: 30]
  -h, --help               Print help
  -V, --version            Print version

```bash
$ ./sender --help
Asynchronous UDP sender

Usage: sender [OPTIONS] --destination <IP:PORT>

Options:
  -d, --destination <IP:PORT>  Sets the IP and port to send UDP packets
  -r, --rate <PACKETS/SEC>     Sets the target send rate in packets per second [default: 10]
  -s, --size <BYTES>           Sets the packet size in bytes [default: 256]
  -t, --retries <COUNT>        Sets the number of retries for sending packets [default: 3]
      --retry-delay <MS>       Sets the delay between retries in milliseconds [default: 100]
  -w, --window <INTERVALS>     Sets the sliding window size for send rate tracking [default: 10]
  -h, --help                   Print help
  -V, --version                Print version

```bash
$ ./receiver --help
Asynchronous UDP receiver

Usage: receiver [OPTIONS] --listen <IP:PORT>

Options:
  -l, --listen <IP:PORT>    Sets the IP and port to listen for incoming UDP packets
  -b, --buffer <BYTES>      Sets the receive buffer size in bytes [default: 65535]
  -w, --window <INTERVALS>  Sets the sliding window size for receive rate tracking [default: 10]
  -h, --help                Print help
  -V, --version             Print version