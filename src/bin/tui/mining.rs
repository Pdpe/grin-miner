// Copyright 2018 The Grin Developers
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

//! Mining status view definition

use std::cmp::Ordering;
use std::sync::{Arc, RwLock};

use cursive::Cursive;
use cursive::view::View;
use cursive::views::{BoxView, Dialog, LinearLayout, StackView,
                     TextView};
use cursive::direction::Orientation;
use cursive::traits::*;

use tui::constants::*;
use tui::types::*;

use stats;
use util::cuckoo_miner::CuckooMinerDeviceStats;
use tui::table::{TableView, TableViewItem};

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
enum MiningDeviceColumn {
	Plugin,
	DeviceId,
	DeviceName,
	EdgeBits,
	InUse,
	ErrorStatus,
	LastGraphTime,
	GraphsPerSecond,
}

impl MiningDeviceColumn {
	fn _as_str(&self) -> &str {
		match *self {
			MiningDeviceColumn::Plugin => "Plugin",
			MiningDeviceColumn::DeviceId => "Device ID",
			MiningDeviceColumn::DeviceName => "Name",
			MiningDeviceColumn::EdgeBits => "Graph Size",
			MiningDeviceColumn::InUse => "In Use",
			MiningDeviceColumn::ErrorStatus => "Status",
			MiningDeviceColumn::LastGraphTime => "Last Graph Time",
			MiningDeviceColumn::GraphsPerSecond => "GPS",
		}
	}
}

impl TableViewItem<MiningDeviceColumn> for CuckooMinerDeviceStats {
	fn to_column(&self, column: MiningDeviceColumn) -> String {
		let last_solution_time_secs = self.last_solution_time as f64 / 1000000000.0;
		match column {
			MiningDeviceColumn::Plugin => self.plugin_name.clone().unwrap(),
			MiningDeviceColumn::DeviceId => self.device_id.clone(),
			MiningDeviceColumn::DeviceName => self.device_name.clone(),
			MiningDeviceColumn::EdgeBits => self.cuckoo_size.clone(),
			MiningDeviceColumn::InUse => match self.in_use {
				1 => String::from("Yes"),
				_ => String::from("No"),
			},
			MiningDeviceColumn::ErrorStatus => match self.has_errored {
				0 => String::from("OK"),
				_ => String::from("Errored"),
			},
			MiningDeviceColumn::LastGraphTime => {
				String::from(format!("{}s", last_solution_time_secs))
			}
			MiningDeviceColumn::GraphsPerSecond => {
				String::from(format!("{:.*}", 4, 1.0 / last_solution_time_secs))
			}
		}
	}

	fn cmp(&self, other: &Self, column: MiningDeviceColumn) -> Ordering
	where
		Self: Sized,
	{
		let last_solution_time_secs_self = self.last_solution_time as f64 / 1000000000.0;
		let gps_self = 1.0 / last_solution_time_secs_self;
		let last_solution_time_secs_other = other.last_solution_time as f64 / 1000000000.0;
		let gps_other = 1.0 / last_solution_time_secs_other;
		match column {
			MiningDeviceColumn::Plugin => self.plugin_name.cmp(&other.plugin_name),
			MiningDeviceColumn::DeviceId => self.device_id.cmp(&other.device_id),
			MiningDeviceColumn::DeviceName => self.device_name.cmp(&other.device_name),
			MiningDeviceColumn::EdgeBits => self.cuckoo_size.cmp(&other.cuckoo_size),
			MiningDeviceColumn::InUse => self.in_use.cmp(&other.in_use),
			MiningDeviceColumn::ErrorStatus => self.has_errored.cmp(&other.has_errored),
			MiningDeviceColumn::LastGraphTime => {
				self.last_solution_time.cmp(&other.last_solution_time)
			}
			MiningDeviceColumn::GraphsPerSecond => gps_self.partial_cmp(&gps_other).unwrap(),
		}
	}
}

/// Mining status view
pub struct TUIMiningView;

impl TUIStatusListener for TUIMiningView {
	/// Create the mining view
	fn create() -> Box<View> {

		let table_view =
			TableView::<CuckooMinerDeviceStats, MiningDeviceColumn>::new()
				.column(MiningDeviceColumn::Plugin, "Plugin", |c| {
					c.width_percent(15)
				})
				.column(MiningDeviceColumn::DeviceId, "Device ID", |c| {
					c.width_percent(10)
				})
				.column(MiningDeviceColumn::DeviceName, "Device Name", |c| {
					c.width_percent(15)
				})
				.column(MiningDeviceColumn::EdgeBits, "Size", |c| {
					c.width_percent(5)
				})
				.column(MiningDeviceColumn::InUse, "In Use", |c| c.width_percent(5))
				.column(MiningDeviceColumn::ErrorStatus, "Status", |c| {
					c.width_percent(5)
				})
				.column(MiningDeviceColumn::LastGraphTime, "Graph Time", |c| {
					c.width_percent(10)
				})
				.column(MiningDeviceColumn::GraphsPerSecond, "GPS", |c| {
					c.width_percent(10)
				});

		let status_view = LinearLayout::new(Orientation::Vertical)
			.child(
				LinearLayout::new(Orientation::Horizontal)
					.child(TextView::new("Connection Status: Starting...").with_id("mining_server_status")),
			).child(
				LinearLayout::new(Orientation::Horizontal)
					.child(TextView::new("Last Message Sent:  ").with_id("last_message_sent")),
			).child(
				LinearLayout::new(Orientation::Horizontal)
					.child(TextView::new("Last Message Received:  ").with_id("last_message_received")),
			)
			.child(
				LinearLayout::new(Orientation::Horizontal)
					.child(TextView::new("Mining Status: ").with_id("mining_status")),
			)
			.child(
				LinearLayout::new(Orientation::Horizontal)
					.child(TextView::new("  ").with_id("network_info")),
			);

		let mining_device_view = LinearLayout::new(Orientation::Vertical)
			.child(status_view)
			.child(BoxView::with_full_screen(
				Dialog::around(table_view.with_id(TABLE_MINING_STATUS).min_size((50, 20)))
					.title("Mining Devices"),
			))
			.with_id("mining_device_view");

		let view_stack = StackView::new()
			.layer(mining_device_view)
			.with_id("mining_stack_view");

		let mining_view = LinearLayout::new(Orientation::Vertical)
			.child(view_stack);

		Box::new(mining_view.with_id(VIEW_MINING))
	}

	/// update
	fn update(c: &mut Cursive, stats: Arc<RwLock<stats::Stats>>) {
		let stats = stats.read().unwrap();
		let client_stats = stats.client_stats.clone();
		c.call_on_id("mining_server_status", |t: &mut TextView| {
			t.set_content(stats.client_stats.connection_status.clone());
		});
	
		let (basic_mining_status, basic_network_info) = {
			if stats.client_stats.connected {
				if stats.mining_stats.combined_gps == 0.0 {
					(
						"Mining Status: Starting miner and awaiting first graph time...".to_string(),
						" ".to_string(),
					)
				} else {
					(
						format!(
							"Mining Status: Mining at height {} at {:.*} GPS",
							stats.mining_stats.block_height, 4, stats.mining_stats.combined_gps
						),
						format!(
							"Cuck(at)oo - Target Share Difficulty {}",
							stats.mining_stats.target_difficulty.to_string()
						),
					)
				}
			} else {
				("Mining Status: Waiting for server".to_string(), "  ".to_string())
			}
		};
		
		// device
		c.call_on_id("mining_status", |t: &mut TextView| {
			t.set_content(basic_mining_status);
		});
		c.call_on_id("network_info", |t: &mut TextView| {
			t.set_content(basic_network_info);
		});

		c.call_on_id("last_message_sent", |t: &mut TextView| {
			t.set_content(client_stats.last_message_sent.clone());
		});
		c.call_on_id("last_message_received", |t: &mut TextView| {
			t.set_content(client_stats.last_message_received.clone());
		});

		let mining_stats = stats.mining_stats.clone();
		let device_stats = mining_stats.device_stats;

		let mut flattened_device_stats = vec![];

		if device_stats.is_some() {
			let device_stats = device_stats.unwrap();
			for p in device_stats.into_iter() {
				for d in p.into_iter() {
					flattened_device_stats.push(d);
				}
			}
		}

		let _ = c.call_on_id(
			TABLE_MINING_STATUS,
			|t: &mut TableView<CuckooMinerDeviceStats, MiningDeviceColumn>| {
				t.set_items(flattened_device_stats);
			},
		);
	}
}
