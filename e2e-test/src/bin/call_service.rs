// Sends RPCs to the Skir service started by `start_service`.
//
// Run with:
//   cargo run --bin call_service
//
// Make sure the service is running first.

use e2e_test::skir_client::service_client::ServiceClient;
use e2e_test::skirout::base::service::{
    AddUserRequest, GetUserRequest, User, add_user_method, get_user_method,
};

fn main() {
    let client =
        ServiceClient::new("http://127.0.0.1:8787/myapi").expect("failed to create client");

    // ── Add users ─────────────────────────────────────────────────────────────
    let users = vec![
        User {
            name: "Alice".to_owned(),
            quote: "To infinity and beyond!".to_owned(),
            ..Default::default()
        },
        User {
            name: "Bob".to_owned(),
            quote: "May the force be with you.".to_owned(),
            ..Default::default()
        },
        User {
            name: "Carol".to_owned(),
            quote: "It's a trap!".to_owned(),
            ..Default::default()
        },
    ];

    println!("── Adding users ─────────────────────────────────────────────────");
    for user in &users {
        let req = AddUserRequest {
            user: user.clone(),
            _unrecognized: None,
        };
        match client.invoke_remote(add_user_method(), &req, &[]) {
            Ok(_) => println!("  Added: {}", user.name),
            Err(e) => eprintln!("  Error adding {}: {e}", user.name),
        }
    }

    // ── Retrieve users by ID ──────────────────────────────────────────────────
    println!();
    println!("── Fetching users ───────────────────────────────────────────────");
    for id in [1i32, 2, 3, 99] {
        let req = GetUserRequest {
            user_id: id,
            _unrecognized: None,
        };
        match client.invoke_remote(get_user_method(), &req, &[]) {
            Ok(resp) => match resp.user {
                Some(u) => println!("  [{id}] {} — \"{}\"", u.name, u.quote),
                None => println!("  [{id}] <not found>"),
            },
            Err(e) => eprintln!("  [{id}] error: {e}"),
        }
    }
}
