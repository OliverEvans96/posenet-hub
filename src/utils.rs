use async_std::channel;
use std::error::Error;
use std::fmt::Debug;

pub async fn read_rx<T: Debug>(name: &str, rx: channel::Receiver<T>) -> Result<(), Box<dyn Error>> {
    println!("Listening for {}", name);
    let mut i = 0;
    loop {
        let thing = rx.recv().await?;
        i += 1;
        println!("#{} {}: {:#?}", i, name, thing);
    }
}