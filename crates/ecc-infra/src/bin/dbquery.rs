use rusqlite::Connection;

fn main() {
    let conn = Connection::open("ecc.db").unwrap();

    // Find the actual provider model
    let mut stmt = conn.prepare(
        "SELECT id, provider_name, target_model, length(thinking_text) FROM session_records ORDER BY id DESC LIMIT 10"
    ).unwrap();
    let rows: Vec<_> = stmt.query_map([], |r| {
        Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,i64>(3)?))
    }).unwrap().filter_map(|r| r.ok()).collect();

    println!("=== Recent records by provider ===");
    for (id, prov, model, tlen) in &rows {
        println!("Record {} | {} / {} | thinking={}", id, prov, model, tlen);
    }

    // Check one response with thinking to see format
    let mut stmt2 = conn.prepare(
        "SELECT response_body FROM session_records WHERE length(thinking_text) > 0 ORDER BY id DESC LIMIT 1"
    ).unwrap();
    if let Ok(rb) = stmt2.query_row([], |r| r.get::<_,String>(0)) {
        println!("\n=== Sample thinking response (first 500 chars) ===");
        println!("{}", &rb[..rb.len().min(500)]);
    }
}
