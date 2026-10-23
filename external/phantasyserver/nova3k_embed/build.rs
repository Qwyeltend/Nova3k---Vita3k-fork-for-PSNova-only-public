// Nova3K: the compiled server data (com_data.mp, made by data_compiler from ../data) is built into the
// library. CI runs the data compiler first and passes the file in NOVA3K_COM_DATA.
fn main() {
    println!("cargo:rerun-if-env-changed=NOVA3K_COM_DATA");
    let path = std::env::var("NOVA3K_COM_DATA").expect("set NOVA3K_COM_DATA to the compiled com_data.mp");
    println!("cargo:rerun-if-changed={path}");
}
