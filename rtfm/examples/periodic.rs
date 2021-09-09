#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![deny(warnings)]
#![no_main]

use core::time::Duration;

#[rtfm::app]
const APP: () = {
    #[init(spawn = [foo])]
    fn init(c: init::Context) {
        c.spawn.foo().ok();
    }

    #[task(schedule = [foo])]
    fn foo(c: foo::Context) {
        static mut COUNT: u8 = 0;

        print!(".");

        *COUNT += 1;
        if *COUNT >= 3 {
            print!("\n");
            std::process::exit(0);
        }

        c.schedule.foo(c.scheduled + Duration::from_secs(1)).ok();
    }
};
