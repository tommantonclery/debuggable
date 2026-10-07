#[derive(debuggable::Debuggable)]
struct S {
    #[debuggable(items, text)]
    bytes: Vec<u8>,
}

fn main() {}
