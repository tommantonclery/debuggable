#[derive(debuggable::Debuggable)]
#[debuggable(summary = "token")]
enum Token {
    Ident(String),
    Eof,
}

fn main() {}
