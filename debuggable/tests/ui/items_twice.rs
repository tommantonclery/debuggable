#[derive(debuggable::Debuggable)]
struct Two {
    #[debuggable(items)]
    a: Vec<u8>,
    #[debuggable(items)]
    b: Vec<u8>,
}

fn main() {}
