use std::ffi::OsStr;

use anyhow::Result;
use libloading::{Library, Symbol};

pub trait Plugin {
    fn run(&self);
}

#[derive(Default)]
pub struct PluginManager {
    plugins: Vec<Box<dyn Plugin>>,
    libraries: Vec<Library>,
}

impl PluginManager {
    pub unsafe fn load_plugin(&mut self, filename: impl AsRef<OsStr>) -> Result<()> {
        let library = Library::new(filename)?;
        let plugin_create: Symbol<unsafe fn() -> *mut dyn Plugin> =
            library.get(b"plugin_create")?;

        let plugin = plugin_create();
        let plugin = Box::from_raw(plugin);

        self.plugins.push(plugin);
        self.libraries.push(library);

        Ok(())
    }

    pub fn run(&self) {
        self.plugins.iter().for_each(|plugin| plugin.run());
    }
}
