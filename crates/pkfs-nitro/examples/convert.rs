fn main() {
    let path = std::env::args().nth(1).expect("usage: convert <nsbmd>");
    let buf = std::fs::read(&path).unwrap();
    for m in pkfs_nitro::buffers_to_glbs(vec![buf]).unwrap() {
        let out = format!("scratch/nitro_{}.glb", m.name);
        std::fs::write(&out, &m.bytes).unwrap();
        println!("{out}: {} bytes", m.bytes.len());
    }
}
