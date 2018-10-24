// Copyright 2017 The Grin Developers
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Main interface for callers into cuckoo-miner. Provides functionality
//! to load a mining plugin, send it a Cuckoo Cycle POW problem, and
//! return any resulting solutions.

use std::sync::{mpsc, Arc, RwLock};
use std::{thread, time};
use util::LOGGER;

use config::types::PluginConfig;
use miner::types::{
	JobSharedData, JobSharedDataType,
	SolverInstance,
};

use miner::util;
use {CuckooMinerError, PluginLibrary, SolverStats, SolverSolutions};

/// Miner control Messages

enum ControlMessage {
	/// Stop everything, pull down, exis
	Stop,
	/// Stop current mining iteration, set solver threads to paused
	Pause,
	/// Resume
	Resume,
}

/// An instance of a miner, which loads a cuckoo-miner plugin
/// and calls its mine function according to the provided configuration

pub struct CuckooMiner {
	/// Configurations
	configs: Vec<PluginConfig>,

	/// Data shared across threads
	pub shared_data: Arc<RwLock<JobSharedData>>,

	/// Job control tx
	control_txs: Vec<mpsc::Sender<ControlMessage>>,

	/// solver loop tx
	solver_loop_txs: Vec<mpsc::Sender<ControlMessage>>,
}

impl CuckooMiner {
	/// Creates a new instance of a CuckooMiner with the given configuration.
	/// One PluginConfig per device

	pub fn new(configs: Vec<PluginConfig>) -> CuckooMiner {
		let len = configs.len();
		CuckooMiner {
			configs: configs,
			shared_data: Arc::new(RwLock::new(JobSharedData::new(len))),
			control_txs: vec![],
			solver_loop_txs: vec![],
		}
	}

	/// Solver's instance of a thread
	fn solver_thread(
		mut solver: SolverInstance,
		instance: usize,
		shared_data: JobSharedDataType,
		control_rx: mpsc::Receiver<ControlMessage>,
		solver_loop_rx: mpsc::Receiver<ControlMessage>,
	) {
		// "Detach" a stop function from the solver, to let us keep a control thread going
		let stop_fn = solver.lib.get_stop_solver_instance();
		let sleep_dur = time::Duration::from_millis(100);
		// monitor whether to send a stop signal to the solver, which should
		// end the current solve attempt below
		let stop_handle = thread::spawn(move || {
			loop {
				while let Some(message) = control_rx.try_iter().next() {
					match message {
						ControlMessage::Stop => {
							PluginLibrary::stop_solver_from_instance(stop_fn.clone());
							return;
						},
						ControlMessage::Pause => {
							PluginLibrary::stop_solver_from_instance(stop_fn.clone());
						},
						_ => {},
					};
				}
			}
		});

		let mut iter_count = 0;
		let ctx = solver.lib.create_solver_ctx(&mut solver.config.params);
		let mut paused = true;
		loop {
			if let Some(message) = solver_loop_rx.try_iter().next() {
				match message {
					ControlMessage::Stop => break,
					ControlMessage::Pause => paused = true,
					ControlMessage::Resume => paused = false,
				}
			}
			if paused {
				thread::sleep(sleep_dur);
				continue;
			}
			{
				let mut s = shared_data.write().unwrap();
				s.stats[instance].set_plugin_name(&solver.config.name);
			}
			let header_pre = {
				shared_data.read().unwrap().pre_nonce.clone()
			};
			let header_post = {
				shared_data.read().unwrap().post_nonce.clone()
			};
			let header = util::get_next_header_data(&header_pre, &header_post);
			let nonce = header.0;
			solver.lib.run_solver(
				ctx,
				header.1,
				0,
				1,
				&mut solver.solutions,
				&mut solver.stats,
			);
			iter_count += 1;
			{
				let mut s = shared_data.write().unwrap();
				s.stats[instance] = solver.stats.clone();
				s.stats[instance].iterations = iter_count;
				if solver.solutions.num_sols > 0 {
					for mut ss in solver.solutions.sols.iter_mut() {
						ss.nonce = nonce;
					}
					s.solutions.push(solver.solutions.clone());
				}
			}
			solver.solutions = SolverSolutions::default();
		}

		let _ = stop_handle.join();
		solver.lib.destroy_solver_ctx(ctx);
		solver.lib.unload();
	}

	/// Starts solvers, ready for jobs via job control
	pub fn start_solvers(
		&mut self,
	) -> Result<(), CuckooMinerError> {
		let mut solvers = Vec::new();
		for c in self.configs.clone() {
			solvers.push(SolverInstance::new(c)?);
		}
		let mut i = 0;
		for s in solvers {
			let sd = self.shared_data.clone();
			let (control_tx, control_rx) = mpsc::channel::<ControlMessage>();
			let (solver_tx, solver_rx) = mpsc::channel::<ControlMessage>();
			self.control_txs.push(control_tx);
			self.solver_loop_txs.push(solver_tx);
			thread::spawn(move || {
				let _ = CuckooMiner::solver_thread(s, i, sd, control_rx, solver_rx);
			});
			i += 1;
		}
		Ok(())
	}

	/// An asynchronous -esque version of the plugin miner, which takes
	/// parts of the header and the target difficulty as input, and begins
	/// asyncronous processing to find a solution. The loaded plugin is
	/// responsible
	/// for how it wishes to manage processing or distribute the load. Once
	/// called
	/// this function will continue to find solutions over the target difficulty
	/// for the given inputs and place them into its output queue until
	/// instructed to stop.

	pub fn notify(
		&mut self,
		job_id: u32,      // Job id
		pre_nonce: &str,  // Pre-nonce portion of header
		post_nonce: &str, // Post-nonce portion of header
		difficulty: u64,  /* The target difficulty, only sols greater than this difficulty will
		                   * be returned. */
	) -> Result<(), CuckooMinerError> {
		// stop/pause any existing jobs
		self.pause_solvers();
		// Notify of new header data
		{
			let mut sd = self.shared_data.write().unwrap();
			sd.job_id = job_id;
			sd.pre_nonce = pre_nonce.to_owned();
			sd.post_nonce = post_nonce.to_owned();
			sd.difficulty = difficulty;
		}
		// resume jobs
		self.resume_solvers();
		Ok(())
	}

	/// Returns solutions if currently waiting.

	pub fn get_solutions(&self) -> Option<SolverSolutions> {
		// just to prevent endless needless locking of this
		// when using fast test miners, in real cuckoo30 terms
		// this shouldn't be an issue
		// TODO: Make this less blocky
		thread::sleep(time::Duration::from_millis(10));
		// let time_pre_lock=Instant::now();
		{
			let mut s = self.shared_data.write().unwrap();
		// let time_elapsed=Instant::now()-time_pre_lock;
		// println!("Get_solution Time spent waiting for lock: {}",
		// time_elapsed.as_secs()*1000 +(time_elapsed.subsec_nanos()/1_000_000)as u64);
			if s.solutions.len() > 0 {
				let sol = s.solutions.pop().unwrap();
				return Some(sol);
			}
		}
		None
	}

	/// get stats for all running solvers
	pub fn get_stats(&self) -> Result<Vec<SolverStats>, CuckooMinerError> {
		let s = self.shared_data.read().unwrap();
		Ok(s.stats.clone())
	}

	/// #Description
	///
	/// Stops the current job, and signals for the loaded plugin to stop
	/// processing and perform any cleanup it needs to do.
	///
	/// #Returns
	///
	/// Nothing

	pub fn stop_solvers(&self) {
		for t in self.control_txs.iter() {
			let _ = t.send(ControlMessage::Stop);
		}
		for t in self.solver_loop_txs.iter() {
			let _ = t.send(ControlMessage::Stop);
		}
		debug!(LOGGER, "Stop message sent");
	}

	/// Tells current solvers to stop and wait
	pub fn pause_solvers(&self) {
		for t in self.control_txs.iter() {
			let _ = t.send(ControlMessage::Pause);
		}
		for t in self.solver_loop_txs.iter() {
			let _ = t.send(ControlMessage::Pause);
		}
		debug!(LOGGER, "Pause message sent");
	}

	/// Tells current solvers to stop and wait
	pub fn resume_solvers(&self) {
		for t in self.control_txs.iter() {
			let _ = t.send(ControlMessage::Resume);
		}
		for t in self.solver_loop_txs.iter() {
			let _ = t.send(ControlMessage::Resume);
		}
		debug!(LOGGER, "Resume message sent");
	}
}