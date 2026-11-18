// Isolated positive control for the release no-fault-hook guard: a seam-named function
// that must stay visible in an optimized binary built with the same flags.
mod seams {
    #[inline(never)]
    pub fn hook(value: u32) -> u32 {
        value.wrapping_mul(31)
    }
}
fn main() {
    println!("{}", seams::hook(std::env::args().count() as u32));
}
