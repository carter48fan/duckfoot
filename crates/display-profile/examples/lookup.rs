//! Print what the system has for a monitor: `cargo run -p display-profile --example lookup
//! -- [output name]`.
fn main() {
    let monitor = display_profile::Monitor::Output(std::env::args().nth(1));
    match display_profile::lookup(&monitor) {
        Ok(Some(p)) => println!("{}: {} bytes", p.name, p.icc.len()),
        Ok(None) => println!("no profile for {monitor:?}"),
        Err(e) => println!("could not ask: {e}"),
    }
}
