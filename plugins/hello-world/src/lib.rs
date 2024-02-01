#[derive(Default)]
pub struct Hello {}

impl sora::Plugin for Hello {
    fn run(&self) {
        println!("Hello, World!");
    }
}

#[no_mangle]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn plugin_create() -> *mut dyn sora::Plugin {
    Box::into_raw(Box::new(Hello::default()))
}
