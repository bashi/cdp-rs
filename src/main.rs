use smol::Task;
use structopt::StructOpt;

pub mod endpoints;

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

    let endpoints = endpoints::Endpoints::new(&opt.host, opt.port);

    smol::run(async move {
        let task = Task::spawn(endpoints.clone().version());
        let res = task.await?;
        println!("{:#?}", res);

        let task = Task::spawn(endpoints.clone().target_list());
        let res = task.await?;
        println!("{:#?}", res);

        let task = Task::spawn(endpoints.clone().open_new_tab("https://www.example.com"));
        let target = task.await?;
        println!("{:#?}", target);

        let task = Task::spawn(endpoints.clone().close(target.id.clone()));
        let target = task.await?;
        println!("{:#?}", target);

        Ok(())
    })
}
