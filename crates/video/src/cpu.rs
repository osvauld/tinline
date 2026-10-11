//! Keep codec threads on the fast cores of a big.LITTLE phone.
//!
//! Android lets a foreground app use every core, but the scheduler judges each codec worker
//! thread as lightly loaded and keeps it on the little cores, where a 720p frame takes 3x longer
//! (measured on a moto g52). Pinning the thread that creates the encoder/decoder pins the worker
//! threads it spawns too (affinity is inherited). Apps may do this for their own threads.

/// CPUs with the highest `cpuinfo_max_freq`, or `None` if all cores are the same (or unknown).
pub fn fast_cores() -> Option<Vec<usize>> {
    let mut freq = Vec::new();
    for cpu in 0..64 {
        let p = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_max_freq");
        match std::fs::read_to_string(p) {
            Ok(s) => freq.push((cpu, s.trim().parse::<u64>().ok()?)),
            Err(_) if cpu > 0 && freq.is_empty() => return None,
            Err(_) => continue,
        }
    }
    let max = freq.iter().map(|f| f.1).max()?;
    let fast: Vec<usize> = freq.iter().filter(|f| f.1 == max).map(|f| f.0).collect();
    (fast.len() < freq.len()).then_some(fast)
}

/// Pins the calling thread (and threads it creates afterwards) to `cpus`.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn pin_current_thread(cpus: &[usize]) -> bool {
    // SAFETY: cpu_set_t is plain data; sched_setaffinity(0, ..) affects only this thread.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        for &c in cpus {
            libc::CPU_SET(c, &mut set);
        }
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) == 0
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn pin_current_thread(_cpus: &[usize]) -> bool {
    false
}

/// Pins the calling thread to the fast cores if the device has distinct ones. Returns them.
pub fn pin_to_fast_cores() -> Option<Vec<usize>> {
    let fast = fast_cores()?;
    pin_current_thread(&fast).then_some(fast)
}
