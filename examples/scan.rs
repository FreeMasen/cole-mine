use cole_mine::discover;
use futures::StreamExt;

#[tokio::main]
async fn main() {
    let mut stream = discover(true, false).await.unwrap();
    while let Some(dev) = stream.next().await {
        println!(
            "{}: {}",
            dev.local_name().await.unwrap_or_else(|| "???".to_string()),
            dev.address()
        );
    }
}
