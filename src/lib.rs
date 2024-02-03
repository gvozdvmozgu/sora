#![cfg_attr(test, feature(internal_output_capture))]

use std::any::Any;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::marker::PhantomData;

use ahash::AHashMap;
use libloading::{Library, Symbol};
use rayon::{ThreadPool, ThreadPoolBuilder};

pub type Result<T> = std::result::Result<T, PluginLoadError>;

pub trait Plugin: Any + Send + Sync {
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>().split("::").last().unwrap()
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    fn run(&self);
}

pub trait Loader {
    type Library;

    /// # Safety
    ///
    /// Users of this API must specify the correct type of the function or
    /// variable loaded.
    unsafe fn load(filename: impl AsRef<OsStr>) -> Result<(Self::Library, Box<dyn Plugin>)>;
}

pub struct Native;

impl Loader for Native {
    type Library = Library;

    unsafe fn load(filename: impl AsRef<OsStr>) -> Result<(Self::Library, Box<dyn Plugin>)> {
        let library = Library::new(filename).map_err(PluginLoadError::Library)?;
        let create_plugin: Symbol<unsafe fn() -> *mut dyn Plugin> =
            unsafe { library.get(b"create_plugin").map_err(PluginLoadError::Plugin)? };
        let plugin = Box::from_raw(create_plugin());

        Ok((library, plugin))
    }
}

pub struct PluginManager<L: Loader = Native> {
    plugins: Vec<Box<dyn Plugin>>,
    name_of_plugin: AHashMap<&'static str, usize>,
    libraries: Vec<L::Library>,
    marker: PhantomData<L>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<L: Loader> PluginManager<L> {
    /// # Safety
    ///
    /// Users of this API must specify the correct type of the function or
    /// variable loaded.
    pub unsafe fn load_plugin(&mut self, filename: impl AsRef<OsStr>) -> Result<()> {
        let (library, plugin) = L::load(filename)?;

        self.name_of_plugin.insert(plugin.name(), self.plugins.len());
        self.plugins.push(plugin);
        self.libraries.push(library);

        Ok(())
    }

    pub fn into_dispatcher(mut self) -> Dispatcher<L::Library> {
        use petgraph::algo::toposort;
        use petgraph::graph::DiGraph;

        let mut graph = DiGraph::new();
        let mut node_indices = HashMap::new();
        let mut node = |graph: &mut DiGraph<&str, ()>, name| {
            *node_indices.entry(name).or_insert_with(|| graph.add_node(name))
        };

        for plugin in &self.plugins {
            let master = node(&mut graph, plugin.name());

            for &dependency in plugin.dependencies() {
                let dependency = node(&mut graph, dependency);

                graph.add_edge(dependency, master, ());
            }
        }

        let nodes = toposort(&graph, None).unwrap();
        let mut stages = Vec::with_capacity(nodes.len());

        for node in nodes {
            let index = self.name_of_plugin[graph[node]];
            let plugin = self.plugins.remove(index);

            stages.push(vec![plugin]);
        }

        Dispatcher {
            stages,
            thread_pool: ThreadPoolBuilder::new().build().expect("Invalid configuration"),
            libraries: self.libraries,
        }
    }
}

impl<L: Loader> Default for PluginManager<L> {
    fn default() -> Self {
        Self {
            plugins: <_>::default(),
            name_of_plugin: <_>::default(),
            libraries: <_>::default(),
            marker: PhantomData,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginLoadError {
    #[error("cannot load library for plugin: {0}")]
    Library(libloading::Error),
    #[error("library does not contain a valid plugin")]
    Plugin(libloading::Error),
}

pub struct Dispatcher<L> {
    stages: Vec<Vec<Box<dyn Plugin>>>,
    thread_pool: ThreadPool,
    #[allow(dead_code)]
    libraries: Vec<L>,
}

impl<L> Dispatcher<L> {
    pub fn dispatch(&self) {
        self.stages.iter().for_each(|stage| stage.iter().for_each(|plugin| plugin.run()));
    }
}

impl<L: Send + Sync> Dispatcher<L> {
    pub fn dispatch_par(&self) {
        use rayon::iter::{IntoParallelRefIterator as _, ParallelIterator as _};

        self.thread_pool.install(|| {
            for stage in &self.stages {
                stage.par_iter().for_each(|plugin| plugin.run())
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::sync::Arc;

    use crate::{Loader, Plugin, PluginManager, Result};

    #[macro_export]
    macro_rules! define_plugins {
        ($($name:ident { run: $run_block:block $(, dependencies: [$($deps:expr),*] )? }),+) => {
            $(
                struct $name;
                impl Plugin for $name {
                    fn run(&self) {
                        $run_block
                    }

                    fn dependencies(&self) -> &'static [&'static str] {
                        &[$($($deps),*)?]
                    }
                }
            )+

            pub struct PluginLoader;

            impl Loader for PluginLoader {
                type Library = ();

                unsafe fn load(filename: impl AsRef<OsStr>) -> Result<(Self::Library, Box<dyn Plugin>)> {
                    let name = filename.as_ref().to_str().unwrap();
                    let plugin: Box<dyn Plugin> = match name {
                        $( stringify!($name) => Box::new($name {}), )+
                        _ => unimplemented!(),
                    };

                    Ok(((), plugin))
                }
            }
        };
    }

    #[test]
    fn smoke() {
        define_plugins! {
            A {
                run: {
                    println!("A");
                }
            },
            B {
                run: {
                    println!("B");
                },
                dependencies: ["A"]
            }
        }

        let mut manager: PluginManager<PluginLoader> = PluginManager::default();

        unsafe { manager.load_plugin("B").unwrap() };
        unsafe { manager.load_plugin("A").unwrap() };

        let dispatcher = manager.into_dispatcher();

        std::io::set_output_capture(Some(Default::default()));

        dispatcher.dispatch();

        let captured = std::io::set_output_capture(None);
        let captured = captured.unwrap();
        let captured = Arc::try_unwrap(captured).unwrap();
        let captured = captured.into_inner().unwrap();
        let captured = String::from_utf8(captured).unwrap();

        assert_eq!(captured, "A\nB\n");
    }

    #[test]
    #[should_panic(expected = "Cycle(NodeIndex(1))")]
    fn cycle() {
        define_plugins! {
            A {
                run: {},
                dependencies: ["B"]
            },
            B {
                run: {},
                dependencies: ["A"]
            }
        };

        let mut manager: PluginManager<PluginLoader> = PluginManager::default();

        unsafe { manager.load_plugin("A").unwrap() };
        unsafe { manager.load_plugin("B").unwrap() };

        let _dispatcher = manager.into_dispatcher();
    }
}
