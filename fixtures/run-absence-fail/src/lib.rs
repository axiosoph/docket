pub fn handle_response() -> String {
    // The header came back — the fixture's whole point: this is what the
    // doc's absence claim is supposed to catch.
    "Retry-After".to_string()
}
