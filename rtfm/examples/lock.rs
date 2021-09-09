#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![deny(warnings)]
#![feature(proc_macro_hygiene)]
#![no_main]

use std::process;

#[rtfm::app]
const APP: () = {
    static mut SHARED: u128 = 0;

    #[init(spawn = [foo])]
    fn init(c: init::Context) {
        let rsp = &mut 0; // snapshot of the stack pointer
        println!("A(%rsp={:?})", rsp as *mut _);

        c.spawn.foo().ok();
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        process::exit(0);
    }

    #[task(priority = 1, resources = [SHARED], spawn = [bar, baz])]
    fn foo(mut c: foo::Context) {
        println!("B(%rsp={:?})", &mut 0 as *mut _);

        let spawn = c.spawn;
        c.resources.SHARED.lock(|shared| {
            *shared += 1;

            spawn.bar().ok();

            println!("C(SHARED={})", *shared as u64);

            spawn.baz().ok();
        });

        println!("F");
    }

    #[task(priority = 2, resources = [SHARED])]
    fn bar(c: bar::Context) {
        *c.resources.SHARED += 1;

        println!(
            "E(%rsp={:?}, SHARED={})",
            &mut 0 as *mut _, *c.resources.SHARED as u64,
        );
    }

    #[task(priority = 3)]
    fn baz(_: baz::Context) {
        println!("D(%rsp={:?})", &mut 0 as *mut _);
    }
};
