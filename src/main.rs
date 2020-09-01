use structopt::StructOpt;

mod cli;
mod endpoints;
mod websocket;
mod websocket_target;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "cdp-cli",
    about = "A commandline tool for Chrome DevTools Protocol"
)]
struct Opt {
    #[structopt(long, default_value = "localhost")]
    host: String,
    #[structopt(long, default_value = "9222")]
    port: u16,
}

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

fn main() -> Result<(), Error> {
    let opt = Opt::from_args();
    smol::run(cli::run_repl(opt))
}
