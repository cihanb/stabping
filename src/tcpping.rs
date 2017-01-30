/*
 * Copyright 2016-2017 icasdri
 *
 * This file is part of stabping. The original source code for stabping can be
 * found at <https://github.com/icasdri/stabping>. See COPYING for licensing
 * details.
 */

use std::collections::BTreeSet;
use std::thread;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

use std::time::Duration;
use time::precise_time_ns;
use chrono::Local;

use std::net::TcpStream;

use data::DiscreteDataOnDisk;
use persist::TargetManager;

/**
 * Runs the TCP Ping target's data-collection worker.
 */
pub fn run_tcpping_worker(manager: Arc<TargetManager>,
                          results_out: Sender<(usize, BTreeSet<DiscreteDataOnDisk>)>) -> thread::JoinHandle<()> {
    // start a new thread for the worker
    thread::spawn(move || {
        let mut handles = Vec::new();

        // continue to collect data forever
        loop {
            // retrieve the target's current options
            let (dur_interval, num_addrs) = {
                let ref opt = manager.options_read();
                (
                    Duration::from_millis(opt.interval as u64),
                    opt.addrs.len(),
                )
            };

            // get the current time (to timestamp this round of data with)
            let timestamp: u32 = Local::now().timestamp() as u32;

            let ref t_opt = manager.options_read();
            for addr_i in t_opt.addrs.iter() {
                /*
                 * create channels so the per-addr threads can send back
                 * their data to the worker thread
                 */
                let (tx, rx) = channel();
                handles.push(rx);

                /*
                 * obtain the address string from the address index
                 */
                let addr_str = manager.index_read().get_addr(*addr_i).unwrap();

                /*
                 * spawn a thread to actually collect the data for each
                 * separate address
                 */
                thread::spawn(move || {
                    let start = precise_time_ns();

                    let dur = if TcpStream::connect(addr_str.as_str()).is_ok() {
                        precise_time_ns() - start
                    } else {
                        std::f32::NAN
                    };

                    /*
                     * send back milli-second duration
                     *
                     * we don't care if send fails as that likely means
                     * we took too long and the control thread is no longer
                     * waiting for us
                     */
                    let _ = tx.send(((dur / 100) as f32) / 10_000.);
                });
            }

            /*
             * wait out the designated data-collectiong interval, while giving
             * the per-addr subthreads the entire interval of time to come back
             */
            thread::sleep(dur_interval);

            let data = BTreeSet::new();

            // read back the data from the per-addr subthreads
            for (i, h) in handles.drain(..).enumerate() {
                data.insert(DiscreteDataOnDisk {
                    time: timestamp,
                    index: i as u32,
                    val: match h.recv() {
                        Ok(v) => v,
                        Err(_) => std::f32::NAN,
                    }
                });
            }

            // send off our results to the main thread
            if results_out.send((manager.kind.kind_id(), data)).is_err() {
                println!("Worker Control: failed to send final results back.");
            }
        }
    })
}

