use cole_mine::discover;

#[tokio::main]
async fn main() {
    for dev in discover(true).await.unwrap() {
        println!(
            "{}: {}",
            dev.local_name().await.unwrap_or_else(|| "???".to_string()),
            dev.address()
        );
    }
}
