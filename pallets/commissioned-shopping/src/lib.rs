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

use codec::{Decode, Encode};
use frame_support::{
    decl_error, decl_event, decl_module, decl_storage, dispatch,
    sp_runtime::{traits::Saturating, RuntimeDebug, SaturatedConversion},
    traits::{Currency, EnsureOrigin, Get, OnUnbalanced, ReservableCurrency},
    Parameter,
};
use frame_system::ensure_signed;
use pallet_support::MultiCurrencyAccounting;
use primitives;
use sp_runtime::traits::{AtLeast32BitUnsigned, Member};
use sp_std::prelude::*;

/// Alias for the multi-currency provided balance type
type BalanceOf<T> = <<T as Trait>::Currency as MultiCurrencyAccounting>::Balance;
type NegativeImbalanceOf<T> =
    <<T as Trait>::Currency as MultiCurrencyAccounting>::NegativeImbalance;

/// Configure the pallet by specifying the parameters and types on which it depends.
pub trait Trait: frame_system::Trait {
    /// Because this pallet emits events, it depends on the runtime's definition of an event.
    type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;

    /// Type for identifying assets
    type AssetId: Parameter + Member + AtLeast32BitUnsigned + Default + Copy + Into<u64>;

    /// 货币类型
    /// 系统支持的货币
    type Currency: MultiCurrencyAccounting<AccountId = Self::AccountId, CurrencyId = Self::AssetId>;

    /// 购物时限
    /// 当代购者接受了代购订单而超过购物限制时间，订单自动关闭，代购者的保证金支付给消费者，锁定的支付金额和小费退回消费者。
    type ShoppingTimeLimit: Get<Self::BlockNumber>;

    /// 确认收货时限
    /// 当消费者超过了确认收货的限制时间，订单自动完成，支付金额和小费转给代购者。
    type ReceivingTimeLimit: Get<Self::BlockNumber>;

    /// 回应退货时限
    /// 当消费者申请退货，但代购者超过了回应退货限制时间，订单自动关闭，代购者的保证金支付给消费者，锁定的支付金额和小费退回消费者。
    type ResponseReturnTimeLimit: Get<Self::BlockNumber>;

    /// 接受退货时限
    /// 当消费者超过了提交退货运单的限制时间，订单自动完成，支付金额和小费转给代购者。
    type AcceptReturnTimeLimit: Get<Self::BlockNumber>;

    /// 收入所得者
    /// 交易单有保证金和小费等额外收入，收入所得者有系统配置，一般为国库。
    type OnTransactionPayment: OnUnbalanced<NegativeImbalanceOf<Self>>;

    /// 批准来源
    /// 该模块的一些内部参数调整需要上级批准，一般是理事会或sudo控制者。
    type ApproveOrigin: EnsureOrigin<Self::Origin>;
}

/// 订单状态
#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug)]
pub enum OrderStatus {
    /// 消费发布代购订单，订单委托中，等待代购者接单。
    Pending,
    /// 代购者已接受代购订单。
    Accepted,
    /// 代购者已购买商品并发货中。
    Shipping,
    /// 消费者已确认收货。
    Received,
    /// 消费者手动关闭订单。
    Closed,
    /// 代购者购物失败。
    Failed,
}

/// 商品退货状态
#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug)]
pub enum ReturnStatus {
    /// 消费者申请退货。
    Applied,
    /// 代购者接受商品退货。
    Accepted,
    /// 代购者不接受商品退货。
    Refused,
    /// 消费者发出要退货的商品。
    Shipping,
    /// 代购者确认退货成功。
    Returned,
    /// 消费者取消退货。
    Canceled,
    /// 代购者没有回应退货而导致交易失败。
    NoResponse,
}

/// 订单失败原因
#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug)]
pub enum OrderFailedReason {
    /// 消费者申请退货。
    Applied,
    /// 代购者接受商品退货。
    Accepted,
    /// 代购者不接受商品退货。
    Refused,
    /// 消费者发出要退货的商品。
    Shipping,
    /// 代购者确认退货成功。
    Returned,
    /// 操作超时而导致交易失败。
    Failed,
}

/// 订单信息
/// 代购交易订单信息
#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug)]
pub struct OrderInfo<AccountId, Balance, AssetId, BlockNumber, Hash> {
    consumer: AccountId,       // 消费者账户地址
    shopping_agent: AccountId, // 	代购者账户地址
    payment_amount: Balance,   // 	支付金额
    tip: Balance,              // 	小费
    currency: AssetId,         // 	支付币种
    status: OrderStatus,       // 	订单状态
    create_time: BlockNumber,  // 	提交时间
    required_deposit: Balance, // 	保证金要求
    required_credit: u64,      // 	信用值要求
    shipping_hash: Hash,       // 	发货运单证明
    is_return: bool,           // 	是否有申请退货
    version: u32,              // 	交易单版本
}

/// 退货信息
#[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug)]
pub struct ReturnInfo<Balance, BlockNumber, Hash> {
    return_amount: Balance,   //退货金额
    status: ReturnStatus,     //退货状态
    create_time: BlockNumber, //提交时间
    shipping_hash: Hash,      //退货运单证明
}

decl_storage! {

    trait Store for Module<T: Trait> as CommissionedShopping {

        /// 已完成订单总数
        /// 纪录全网已完成的订单总数。
        pub TotalCompletedOrdersNum get(fn get_total_completed_orders_num): u128;

        /// 当前未完成的订单总数
        /// 纪录全网当前未完成的订单总数。
        pub CurrentUncompletedOrdersNum get(fn get_current_uncompleted_orders_num): u128;

        /// 保证金收入比例
        /// 当代购者违规会被扣除交易设置的保证金，这个额外的收入会按MarginIncomeRatio计算一部分发送给ExtraIncomeEarner，剩余的发送给消费者。
        pub MarginIncomeRatio get(fn get_margin_income_ratio): (u8, u8);

        /// 小费收入比例
        /// 当代购交易单完成后，小费收入会按TipIncomeRatio计算一部分发送给ExtraIncomeEarner，剩余的发送给消费者。
        pub TipIncomeRatio get(fn get_tip_income_ratio): (u8, u8);

        /// 代购交易订单表
        /// 纪录消费者的申请退货的订单信息。
        pub CommissionedShoppingOrders get(fn get_commissioned_shopping_orders):
            double_map hasher(twox_64_concat) T::AccountId,
            hasher(identity) T::Hash => Option<OrderInfo<T::AccountId, BalanceOf<T>, T::AssetId, T::BlockNumber, T::Hash>>;

        /// 各类货币累计成交的金额
        /// 纪录每种货币的购物总金额。
        pub TotalShoppingAmountOfCurrenies get(fn get_total_shopping_amount_of_currenies): map hasher(twox_64_concat) T::AssetId => BalanceOf<T>;

        /// 交易申诉
        /// 纪录每种货币的购物总金额。
        pub AppealOf get(fn get_appeal_of): map hasher(identity) T::Hash => Option<(T::AccountId, T::Hash)>;



    }
}

// Pallets use events to inform users when important changes are made.
// https://substrate.dev/docs/en/knowledgebase/runtime/events
decl_event!(
    pub enum Event<T>
    where
        AccountId = <T as frame_system::Trait>::AccountId,
        Balance = BalanceOf<T>,
        Hash = <T as frame_system::Trait>::Hash,
        AssetId = <T as Trait>::AssetId,
    {
        /// ApplyShoppingOrder
        /// - consumer AccountId 消费者地址
        /// - payment_amount Balance 支付金额
        /// - tip Balance 小费
        /// - currency AssetId 支付币种
        /// - required_deposit Balance 保证金要求
        /// - required_credit u64 信用值要求
        /// - version u32 交易单版本
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - status OrderStatus 订单状态
        ApplyShoppingOrder(
            AccountId,
            Balance,
            Balance,
            AssetId,
            Balance,
            u64,
            u32,
            Hash,
            OrderStatus,
        ),

        /// CloseShoppingOrder
        /// - consumer AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - status OrderStatus 订单状态
        CloseShoppingOrder(AccountId, Hash, OrderStatus),

        /// ShoppingOrderFailed 代购者超时执行代购，导致失败，
        /// - consumer AccountId 消费者地址
        /// - shopping_agent AccountId 代购者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - status OrderStatus 订单状态
        /// - currency AssetId 支付币种
        /// - required_deposit Balance 要求代购者赔偿给消费的保证金
        /// - payment_amount Balance 解锁给消费者的支付金额
        /// - tip Balance 解锁给消费者的小费金额
        ShoppingOrderFailed(
            AccountId,
            AccountId,
            Hash,
            OrderStatus,
            AssetId,
            Balance,
            Balance,
            Balance,
        ),

        /// AcceptShoppingOrder
        /// - shopping_agent AccountId 代购者地址
        /// - consumer AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - status OrderStatus 订单状态
        AcceptShoppingOrder(AccountId, AccountId, Hash, OrderStatus),

        /// DoCommodityShipping
        /// - shopping_agent AccountId 代购者地址
        /// - consumer AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - shipping_hash H256 发货运单证明
        /// - status OrderStatus 订单状态
        DoCommodityShipping(AccountId, AccountId, Hash, Hash, OrderStatus),

        /// ConfirmCommodityReceived 确认商品收货
        /// - consumer AccountId 代购者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - status OrderStatus 订单状态
        ConfirmCommodityReceived(AccountId, Hash, OrderStatus),

        /// ShoppingOrderCompleted 订单完成
        /// - consumer AccountId 代购者地址
        /// - shopping_agent AccountId 代购者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - currency AssetId 支付币种
        /// - payment_amount Balance 支付给代购者金额
        /// - tip Balance 支付给代购者小费
        ShoppingOrderCompleted(AccountId, AccountId, Hash, AssetId, Balance, Balance),

        /// ApplyCommodityReturn
        /// - consumer AccountId 代购者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        /// - status ReturnStatus 商品退货状态
        ApplyCommodityReturn(AccountId, Hash, Hash, ReturnStatus),

        /// AcceptCommodityReturn 接受商品退货
        /// - shopping_agent AccountId 代购者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        /// - status ReturnStatus 商品退货状态
        AcceptCommodityReturn(AccountId, Hash, Hash, ReturnStatus),

        /// RejectCommodityReturn 拒绝商品退货
        /// - shopping_agent AccountId 代购者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        /// - status ReturnStatus 商品退货状态
        RejectCommodityReturn(AccountId, Hash, Hash, ReturnStatus),

        /// NoResponseCommodityReturn 代购者没有回应退货申请
        /// - consumer AccountId 消费者地址
        /// - shopping_agent AccountId 代购者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        /// - status ReturnStatus 商品退货状态
        /// - currency AssetId 支付币种
        /// - required_deposit Balance 要求代购者赔偿给消费的保证金
        /// - refund_amount Balance 商品退款金额
        NoResponseCommodityReturn(
            AccountId,
            AccountId,
            Hash,
            Hash,
            ReturnStatus,
            AssetId,
            Balance,
            Balance,
        ),

        /// DoCommodityReturning
        /// - consumer AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        /// - shipping_hash H256 发货运单证明
        /// - status ReturnStatus 商品退货状态
        DoCommodityReturning(AccountId, Hash, Hash, Hash, ReturnStatus),

        /// ConfirmCommodityReturned
        /// - shopping_agent AccountId 代购者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        /// - status ReturnStatus 商品退货状态
        ConfirmCommodityReturned(AccountId, Hash, Hash, ReturnStatus),

        /// CommodityRefund 商品退款
        /// - consumer AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        /// - status ReturnStatus 商品退货状态
        /// - refund_amount Balance 解锁给消费者的退款金额
        CommodityRefund(AccountId, Hash, Hash, ReturnStatus, Balance),

        /// Appeal
        /// - prosecutor AccountId 原告人
        /// - defendant AccountId 被告人
        /// - owner AccountId 订单拥有者
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - case_hash H256 申诉的案件哈希
        Appeal(AccountId, AccountId, AccountId, Hash, Hash),

        /// RevokeAppeal
        /// - prosecutor AccountId 原告人
        /// - owner AccountId 订单拥有者
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - case_hash H256 申诉的案件哈希
        RevokeAppeal(AccountId, AccountId, Hash, Hash),
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
        /// 外部订单已存在
        ShoppingOrderIsExisted,
        /// 外部订单不存在。
        ShoppingOrderIsNotExisted,
        /// 退货商品已存在
        ReturnedCommodityIsExisted,
        /// 退货商品不存在。
        ReturnedCommodityIsNotExisted,
        /// 必要的保证金不足。
        InsufficientRequiredDeposit,
        /// 必要的信用值不足。
        InsufficientRequiredCredit,
        /// 操作不允许。
        OperationIsNotAllowed,
    }
}

decl_module! {
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {

        type Error = Error<T>;

        fn deposit_event() = default;

        const ShoppingTimeLimit: T::BlockNumber = T::ShoppingTimeLimit::get();

        const ReceivingTimeLimit: T::BlockNumber = T::ReceivingTimeLimit::get();

        const ResponseReturnTimeLimit: T::BlockNumber = T::ResponseReturnTimeLimit::get();

        const AcceptReturnTimeLimit: T::BlockNumber = T::AcceptReturnTimeLimit::get();


        /// apply_shopping_order 发布代购交易单
        /// 所有账户都可以发布代购交易单。
        /// - origin AccountId 消费者地址
        /// - payment_amount Balance 支付金额
        /// - tip Balance 小费
        /// - currency AssetId 支付币种
        /// - required_deposit Balance 保证金要求
        /// - required_credit u64 信用值要求
        /// - version u32 交易单版本
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn apply_shopping_order(origin, payment_amount: BalanceOf<T>, tip: BalanceOf<T>,
            currency: T::AssetId, required_deposit: BalanceOf<T>,
            required_credit: u64, version: u32, ext_order_hash: T::Hash) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }

        /// close_shopping_order 关闭代购交易单
        /// 消费者手动关闭订单。
        /// - origin AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn close_shopping_order(origin, ext_order_hash: T::Hash) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }

        /// accept_shopping_order 接受代购交易单
        /// 所有账户都可以接受代购交易单，成为代购者。
        /// - origin AccountId 代购者地址
        /// - consumer AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn accept_shopping_order(origin, consumer: T::AccountId, ext_order_hash: T::Hash) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }

        /// do_commodity_shipping 提交商品发货信息
        /// 代购者完成电商平台购物，提交商品发货信息
        /// - origin AccountId 代购者地址
        /// - consumer AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - shipping_hash H256 发货运单证明
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn do_commodity_shipping(origin, consumer: T::AccountId, ext_order_hash: T::Hash, shipping_hash: T::Hash) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }

        /// confirm_commodity_received 确认商品收货
        /// 消费者收到商品，提交确认到货。
        /// - origin AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn  confirm_commodity_received(origin, ext_order_hash: T::Hash) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }

        /// apply_commodity_return 申请退货
        /// 消费者申请商品退货
        /// - origin AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn   apply_commodity_return(origin, ext_order_hash: T::Hash, commodity_hash: T::Hash) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }

        /// response_commodity_return 回应退货申请
        /// 代购者回应退货申请
        /// - origin AccountId 代购者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        /// - is_accept bool 是否接受
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn response_commodity_return(origin, ext_order_hash: T::Hash, commodity_hash: T::Hash, is_accept: bool) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }

        /// do_commodity_returning 提交退货商品的运单信息
        /// 消费者执行商品退货，提交运单信息。
        /// - origin AccountId 消费者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        /// - shipping_hash H256 发货运单证明
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn do_commodity_returning(origin, ext_order_hash: T::Hash, commodity_hash: T::Hash, shipping_hash: T::Hash) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }


        /// confirm_commodity_returned 确认退货完成
        /// 代购者确认商家收到退货。
        /// - origin AccountId 代购者地址
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - commodity_hash H256 外部链下订单系统的商品哈希
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn confirm_commodity_returned(origin, ext_order_hash: T::Hash, commodity_hash: T::Hash) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }


        /// appeal 申诉
        /// 代购交易发生纠纷，任一方可以提交申诉。
        /// - origin AccountId 申诉者地址
        /// - owner AccountId 订单拥有者
        /// - ext_order_hash H256 外部链下订单系统的订单哈希
        /// - case_hash H256 申诉的案件哈希
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn appeal(origin, owner: T::AccountId, ext_order_hash: T::Hash, case_hash: T::Hash) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }


        /// revoke_appeal 撤销申诉
        /// 原告账户可以撤销申诉。
        /// - origin AccountId 原告账户
        /// - case_hash H256 案件哈希
        #[weight = 10_000 + T::DbWeight::get().writes(1)]
        pub fn revoke_appeal(origin, case_hash: T::Hash) -> dispatch::DispatchResult {
            // TODO:
            Ok(())
        }

    }
}
