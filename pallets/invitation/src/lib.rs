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

use sp_std::prelude::*;
use codec::{Encode, Decode};
use frame_support::{
	decl_module, decl_storage, decl_event, decl_error, dispatch,
	traits::{Currency, ReservableCurrency, Get},
	sp_runtime::{
		traits::{Saturating},
		RuntimeDebug, SaturatedConversion,
	},
};
use frame_system::ensure_signed;
use primitives;

// #[cfg(test)]
// mod mock;

// #[cfg(test)]
// mod tests;

type BalanceOf<T> = <<T as Trait>::Currency as Currency<<T as frame_system::Trait>::AccountId>>::Balance;

/// Configure the pallet by specifying the parameters and types on which it depends.
pub trait Trait: frame_system::Trait {
	/// Because this pallet emits events, it depends on the runtime's definition of an event.
	type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;

	/// 货币类型
	/// 系统支持的货币
	type Currency: ReservableCurrency<Self::AccountId>;

	/// 最低单位赠金数
	/// 单位赠金数可由邀请人自由设置，但不能低于系统要求的最低值。
	type MinUnitBonus: Get<BalanceOf<Self>>;
}

/// 邀请人信息
/// 存储已登记的邀请人信息
#[derive(Clone, Encode, Decode, Eq, PartialEq, RuntimeDebug)]
pub struct InvitationInfo<Balance> {
	/// 邀请函校验公钥
	pub invitation_pk: primitives::Byte32,
	/// 单笔赠金数`BPG`
	pub unit_bonus: Balance,
	/// 最大邀请人数
	pub max_invitees: u64,
	/// 冻结的金额
	pub frozen_amount: Balance,
	/// 已邀请人数
	pub num_of_invited: u64,
}


decl_storage! {

	trait Store for Module<T: Trait> as Invitation {

		/// 全部邀请人
		/// 当前全网络注册的邀请人
		pub TotalInviters get(fn get_total_inviters): u64;

		/// 已发放的赠金
		/// 全网络累积邀请人发放的赠金总数
		pub TotalDistributedBonus get(fn get_total_distributed_bonus): BalanceOf<T>;

		/// 邀请人登记表
		/// 邀请人绑定其邀请人登记信息。
		pub InviterRegistration get(fn get_inviter_registration): map hasher(twox_64_concat) T::AccountId => Option<InvitationInfo<BalanceOf<T>>>;

	}
}

// Pallets use events to inform users when important changes are made.
// https://substrate.dev/docs/en/knowledgebase/runtime/events
decl_event!(
	pub enum Event<T> where
	AccountId = <T as frame_system::Trait>::AccountId,
	BlockNumber = <T as frame_system::Trait>::BlockNumber,
	Balance = BalanceOf<T>,
	Byte32 = primitives::Byte32,
	Byte64 = primitives::Byte64,
	{

		/// RegisterInviter
		/// - inviter AccountId 邀请人账户地址
		/// - invitation_pk byte32 邀请函校验公钥
		/// - unit_bonus Balance 单笔赠金数
		/// - max_invitees u64 最大邀请人数
		/// - frozen_amount Balance 冻结的金额
		RegisterInviter(AccountId, Byte32, Balance, u64, Balance),

		/// AcceptInvitation
		/// - invitee AccountId 受邀人账户地址
		/// - invitation_sig byte64 邀请函签名
		/// - inviter AccountId 邀请人账户地址
		AcceptInvitation(AccountId, Byte64, AccountId),

		/// EndInvitationPeriod
		/// - inviter AccountId 邀请人账户地址
		/// - reclaimed_bonus Balance 已回收的赠金
		/// - num_of_invited Balance 已邀请人数
		/// - end BlockNumber 准确的结束时间
		EndInvitationPeriod(AccountId, Balance, Balance, BlockNumber),
	}
);

// Errors inform users that something went wrong.
decl_error! {
	pub enum Error for Module<T: Trait> {
		/// Error names should be descriptive.
		NoneValue,
		/// Errors should have helpful documentation associated with them.
		StorageOverflow,
		/// 账户余额不足。
		InsufficientBalance,
		/// 邀请人信息已存在，不可重复提交
		InvitationInfoIsExisted,
		/// 邀请人信息不存在
		InvitationInfoIsNotExisted,
	}
}


decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {

		type Error = Error<T>;

		fn deposit_event() = default;

		const MinUnitBonus: BalanceOf<T> = T::MinUnitBonus::get();

		/// register_inviter
		/// - inviter origin 邀请人账户地址
		/// - invitation_pk byte32 邀请函校验公钥
		/// - unit_bonus Balance 单笔赠金数
		/// - max_invitees u64 最大邀请人数
		#[weight = 10_000 + T::DbWeight::get().writes(1)]
		pub fn register_inviter(origin, invitation_pk: primitives::Byte32, unit_bonus: BalanceOf<T>, max_invitees: u64) -> dispatch::DispatchResult {
			let who = ensure_signed(origin)?;
			// 计算冻结的金额
			let frozen_amount = unit_bonus.saturating_mul(max_invitees.saturated_into());

			//TODO: 判断账户是否有足够余额，并进行锁定

			//登记邀请人信息
			let inviter = InvitationInfo {
				invitation_pk: invitation_pk,
				unit_bonus: unit_bonus,
				max_invitees: max_invitees,
				frozen_amount: frozen_amount,
				num_of_invited: 0,
			};
			InviterRegistration::<T>::insert(&who, inviter);
			// Emit an event.
			Self::deposit_event(RawEvent::RegisterInviter(who, invitation_pk, unit_bonus, max_invitees, frozen_amount));
			Ok(())
		}

		
		/// accept_invitation
		/// - invitee origin 受邀人账户地址
		/// - invitation_sig byte64 邀请函签名
		/// - inviter AccountId 邀请人账户地址
		#[weight = 10_000 + T::DbWeight::get().writes(1)]
		pub fn accept_invitation(origin, invitation_sig: primitives::Byte64, inviter: T::AccountId) -> dispatch::DispatchResult {
			Ok(())
		}

		/// end_invitation_period
		/// - inviter address 邀请人账户地址
		#[weight = 10_000 + T::DbWeight::get().writes(1)]
		pub fn end_invitation_period(origin) -> dispatch::DispatchResult {
			Ok(())
		}
	}
}
