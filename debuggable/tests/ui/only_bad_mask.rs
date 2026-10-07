#[derive(debuggable::Debuggable)]
struct S {
    #[debuggable(items, only = "live & 0")]
    v: Vec<u8>,
}

fn main() {}
