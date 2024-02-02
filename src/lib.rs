use std::any::Any;
use std::ffi::OsStr;

use anyhow::Result;
use libloading::{Library, Symbol};

pub trait Plugin: Any + Send + Sync {
    fn run(&self);
}

#[derive(Default)]
pub struct PluginManager {
    plugins: Vec<Box<dyn Plugin>>,
    libraries: Vec<Library>,
}

impl PluginManager {
    /// # Safety
    ///
    /// Users of this API must specify the correct type of the function or
    /// variable loaded.
    pub unsafe fn load_plugin(
        &mut self,
        filename: impl AsRef<OsStr>,
    ) -> Result<(), PluginLoadError> {
        let library = Library::new(filename).map_err(PluginLoadError::Library)?;
        let create_plugin: Symbol<unsafe fn() -> *mut dyn Plugin> =
            unsafe { library.get(b"create_plugin").map_err(PluginLoadError::Plugin)? };

        let plugin = create_plugin();
        let plugin = Box::from_raw(plugin);

        self.plugins.push(plugin);
        self.libraries.push(library);

        Ok(())
    }

    pub fn par_dispatch(&self) {
        use rayon::iter::{IntoParallelRefIterator as _, ParallelIterator as _};

        self.plugins.par_iter().for_each(|plugin| plugin.run());
    }

    pub fn dispatch(&self) {
        self.plugins.iter().for_each(|plugin| plugin.run());
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginLoadError {
    #[error("cannot load library for plugin: {0}")]
    Library(libloading::Error),
    #[error("library does not contain a valid plugin")]
    Plugin(libloading::Error),
}
