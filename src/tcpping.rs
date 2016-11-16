use std::thread;
use std::sync::mpsc::{channel, Receiver, Sender};

use std::time::Duration;
use time::precise_time_ns;
use chrono::Local;
use chrono::TimeZone;

use std::net::TcpStream;

pub struct TcpPingOptions {
    addresses: Vec<String>,  // Vec of addresses to ping
    interval: u32,  // in millis
    avg_across: u32,  // number of pings to send for each interval (and then take average)
    pause: u32,  // in millis
}

pub struct TcpPingResults {
    time: u32,  // seconds from epoch timestamp
    data: Vec<u32>,  // resulting latencies for each address in micro seconds
}

fn run_tcpping_workers(initial_options: TcpPingOptions,
                       options_in: Receiver<TcpPingOptions>,
                       results_out: Sender<TcpPingResults>) {
    thread::spawn(move || {
        let mut options = initial_options;
        let mut dur_interval = Duration::from_millis(options.interval as u64);
        let mut dur_pause = Duration::from_millis(options.pause as u64);

        let mut handles = Vec::new();

        loop {
            if let Ok(new_options) = options_in.try_recv() {
                options = new_options;
                dur_interval = Duration::from_millis(options.interval as u64);
                dur_pause = Duration::from_millis(options.pause as u64);
            }

            let avg_across = options.avg_across;
            let timestamp: u32 = Local::now().timestamp() as u32;

            for addr in options.addresses.iter() {
                let a = addr.clone();

                let (tx, rx) = channel();
                handles.push(rx);

                thread::spawn(move || {
                    let mut sum = 0;
                    let mut denom = 0;
                    for _ in 0..avg_across {
                        let start = precise_time_ns();
                        if TcpStream::connect(a.as_str()).is_ok() {
                            sum += precise_time_ns() - start;
                            denom += 1;
                        }
                        thread::sleep(dur_pause);
                    }

                    if denom != 0 {
                        /*
                         * send back micro-second average.
                         *
                         * we don't care if send fails as that likely means
                         * we took too long and the control thread is no longer
                         * waiting for us
                         */
                        let _ = tx.send((sum / denom / 1000) as u32);
                    }
                });
            }
            thread::sleep(dur_interval);

            let mut data: Vec<u32> = Vec::with_capacity(options.addresses.len());
            for h in handles.drain(..) {
                if let Ok(val) = h.try_recv() {
                    data.push(val);
                } else {
                    // on error or timeout, hand back a gigantic sentinel value
                    data.push(options.interval);
                }
            }

            if results_out.send(TcpPingResults {time: timestamp, data: data}).is_err() {
                println!("Worker Control: failed to send final results back.");
            }
        }
    });
}


fn main() {
    let (_, options_rx) = channel();
    let (results_tx, results_rx) = channel();

    let master_addresses = vec!["google.com:80".to_owned(),
                                "8.8.8.8:53".to_owned()];
    let addresses = master_addresses.clone();

    run_tcpping_workers(TcpPingOptions {
        addresses: master_addresses,
        interval: 3000,
        avg_across: 3,
        pause: 100,
    }, options_rx, results_tx);

    for res in results_rx.iter() {
        println!("{}", Local.timestamp(res.time as i64, 0));
        for (addr, val) in addresses.iter().zip(res.data) {
            // val comes in as micro seconds
            let whole_ms = val / 1000;
            let part_ms = (val % 1000) / 10;
            println!("Connection to {} took {}.{} ms.", addr, whole_ms, part_ms);
        }
        println!("");
    }
}