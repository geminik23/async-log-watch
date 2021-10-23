use lipsum::lipsum;
use rand::Rng;

fn main() {
    let mut rng = rand::thread_rng();

    loop {
        println!("{}", lipsum(rng.gen_range(100..1000)));
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
