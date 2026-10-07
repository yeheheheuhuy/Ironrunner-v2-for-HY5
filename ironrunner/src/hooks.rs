use anyhow::{Result, anyhow};
use ilhook::x86::{ClosureHookPoint, HookFlags, hook_closure_jmp_back};

use crate::address::Addresses;
use crate::runner::run;

fn hook_input_func<'a>(addresses: Addresses) -> Result<ClosureHookPoint<'a>> {
    let on_call = move |_| {
        run(addresses);
    };
    unsafe {
        hook_closure_jmp_back(
            addresses.input_func,
            on_call,
            ilhook::x86::CallbackOption::None,
            HookFlags::empty(),
        )
        .map_err(|e| anyhow!("{e}"))
    }
}

pub fn init<'a>(addresses: &Addresses) -> Result<Vec<ClosureHookPoint<'a>>> {
    let hooks = vec![hook_input_func(*addresses)?];
    Ok(hooks)
}
