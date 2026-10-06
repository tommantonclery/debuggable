#[derive(debuggable::Debuggable)]
struct Buf {
    #[debuggable(items, len = "count")]
    data: Vec<u8>,
    len: usize,
}

fn main() {}
