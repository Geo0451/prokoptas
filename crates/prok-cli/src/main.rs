use prok_core::{Result, VERSION};

fn run() -> Result<()> {
    println!("prok {VERSION}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{}: {error}", error.code().as_str());
        std::process::exit(1);
    }
}
