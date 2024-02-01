#![feature(exact_size_is_empty)]

use anyhow::{bail, Result};
use sora::PluginManager;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);

    let path = match args.next() {
        Some(filename) if args.is_empty() => filename,
        Some(_) => bail!("only one plugin folder path must be specified."),
        None => bail!("a plugin folder path must be specified."),
    };

    let mut manager = PluginManager::default();

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        unsafe { manager.load_plugin(entry.path())? };
    }

    manager.run();

    Ok(())
}
