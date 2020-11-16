use anyhow::Result;
use structopt::StructOpt;
use swap::{
    alice::{abort, simple_swap, AliceState},
    cli::Options,
    io::Io,
    storage::Database,
};

fn main() {
    let opt = Options::from_args();

    let io: Io = {
        let db = Database::open(std::path::Path::new("./.swap-db/")).unwrap();
        unimplemented!()
    };

    match opt {
        Options::Alice { .. } => {
            simple_swap(AliceState::Started, io);
        }
        Options::Recover { .. } => {
            let stored_state: AliceState = unimplemented!("io.get_state(uuid)?");
            abort(stored_state, io);
        }
        _ => {}
    };
}
