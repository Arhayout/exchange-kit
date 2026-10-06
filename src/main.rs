#![cfg_attr(test, feature(test))]

pub mod data_generator;
pub mod orderbook;
pub mod shared_data;
pub mod signatures;

fn main() {
    println!("exchange-kit — HMAC signing, order-book replay, and lock-free shared state");
    println!("Run `cargo test` and `cargo bench` to exercise the library.");
}
