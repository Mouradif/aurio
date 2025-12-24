use midir::MidiInput;

fn main() {
    let midi_in = MidiInput::new("aurio").expect("failed to create MIDI input");

    println!("Available MIDI inputs:");
    for (i, port) in midi_in.ports().iter().enumerate() {
        println!("  {}: {}", i, midi_in.port_name(port).unwrap_or_default());
    }

    let ports = midi_in.ports();
    let port = ports
        .iter()
        .find(|p| midi_in.port_name(p).unwrap_or_default().contains("APC"))
        .or_else(|| ports.first())
        .expect("no MIDI input found");

    println!(
        "\nConnecting to: {}",
        midi_in.port_name(port).unwrap_or_default()
    );

    let _conn = midi_in
        .connect(
            port,
            "aurio-input",
            |timestamp, message, _| {
                print_midi(timestamp, message);
            },
            (),
        )
        .expect("failed to connect");

    println!("Listening for MIDI. Press Enter to quit.\n");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}

fn print_midi(timestamp: u64, msg: &[u8]) {
    let status = msg[0] & 0xF0;
    let channel = msg[0] & 0x0F;

    match status {
        0x90 if msg[2] > 0 => {
            println!(
                "[{timestamp:>8}] Note ON  ch={channel} note={} vel={}",
                msg[1], msg[2]
            );
        }
        0x80 | 0x90 => {
            println!("[{timestamp:>8}] Note OFF ch={channel} note={}", msg[1]);
        }
        0xB0 => {
            println!(
                "[{timestamp:>8}] CC       ch={channel} ctrl={} val={}",
                msg[1], msg[2]
            );
        }
        0xE0 => {
            let bend = ((msg[2] as u16) << 7) | (msg[1] as u16);
            println!("[{timestamp:>8}] Bend     ch={channel} val={}", bend);
        }
        _ => {
            println!("[{timestamp:>8}] Raw: {:02X?}", msg);
        }
    }
}

