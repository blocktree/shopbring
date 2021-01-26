/*
 * Copyright (C) 2017-2021 blocktree.
 * SPDX-License-Identifier: Apache-2.0
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *  	http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Encode, Decode};
use frame_support::{
	decl_module, decl_storage, decl_event, decl_error, dispatch,
	traits::{Currency, ReservableCurrency, OnUnbalanced, Get, BalanceStatus, EnsureOrigin},
};
use frame_system::ensure_signed;

// #[cfg(test)]
// mod mock;

// #[cfg(test)]
// mod tests;

type BalanceOf<T> = <<T as Trait>::Currency as Currency<<T as frame_system::Trait>::AccountId>>::Balance;

/// Configure the pallet by specifying the parameters and types on which it depends.
pub trait Trait: frame_system::Trait {
	/// Because this pallet emits events, it depends on the runtime's definition of an event.
	type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;

	type Currency: ReservableCurrency<Self::AccountId>;

	type MinUnitBonus: Get<BalanceOf<Self>>; //最低单位赠金数
}


decl_storage! {

	trait Store for Module<T: Trait> as TemplateModule {

		pub Something get(fn something): Option<u32>;

		pub TotalInviters get(fn get_totalInviters): Option<u64>; //全部邀请人

		pub TotalDistributedBonus get(fn get_total_distributed_bonus): Option<u64>; //已发放的赠金

		pub InviterRegistration get(fn get_inviter_registration): map hasher(twox_64_concat) T::AccountId => BalanceOf<T>;

	}
}

// Pallets use events to inform users when important changes are made.
// https://substrate.dev/docs/en/knowledgebase/runtime/events
decl_event!(
	pub enum Event<T> where AccountId = <T as frame_system::Trait>::AccountId {

		SomethingStored(u32, AccountId),

		/// RegisterInviter
		/// - inviter address 邀请人账户地址
		/// - unit_bonus Balance 单笔赠金数
		/// - max_invitees u64 最大邀请人数
		RegisterInviter(AccountId, u64),
	}
);

// Errors inform users that something went wrong.
decl_error! {
	pub enum Error for Module<T: Trait> {
		/// Error names should be descriptive.
		NoneValue,
		/// Errors should have helpful documentation associated with them.
		StorageOverflow,
	}
}


decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {

		type Error = Error<T>;

		fn deposit_event() = default;

		const MinUnitBonus: BalanceOf<T> = T::MinUnitBonus::get();

		/// register_inviter
		/// - inviter address 邀请人账户地址
		/// - unit_bonus Balance 单笔赠金数
		/// - max_invitees u64 最大邀请人数
		#[weight = 10_000 + T::DbWeight::get().writes(1)]
		fn register_inviter(origin, unit_bonus: BalanceOf<T>, max_invitees: u64) -> dispatch::DispatchResult {
			let _who = ensure_signed(origin)?;
			<InviterRegistration<T>>::insert(&_who, unit_bonus);
			// Emit an event.
			Self::deposit_event(RawEvent::RegisterInviter(_who, max_invitees));
			Ok(())
		}
	}
}
