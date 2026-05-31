// Evita una consola extra en Windows release. En Linux no tiene efecto.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    arrow_app_lib::run()
}
