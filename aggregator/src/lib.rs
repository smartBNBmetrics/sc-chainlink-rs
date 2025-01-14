#![no_std]
#![feature(destructuring_assignment)]

elrond_wasm::imports!();
mod aggregator_data;
pub mod aggregator_interface;
pub mod median;

use aggregator_data::{Funds, OracleRoundState, OracleStatus, Requester, RoundDetails, Submission};
use aggregator_interface::Round;

const RESERVE_ROUNDS: u64 = 2;
const ROUND_MAX: u64 = u64::MAX;

#[elrond_wasm_derive::contract]
pub trait Aggregator {
    #[storage_mapper("token_id")]
    fn token_id(&self) -> SingleValueMapper<Self::Storage, TokenIdentifier>;

    // Round related params
    #[storage_mapper("payment_amount")]
    fn payment_amount(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[storage_mapper("max_submission_count")]
    fn max_submission_count(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[storage_mapper("min_submission_count")]
    fn min_submission_count(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[storage_mapper("restart_delay")]
    fn restart_delay(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[storage_mapper("timeout")]
    fn timeout(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[storage_mapper("min_submission_value")]
    fn min_submission_value(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[storage_mapper("max_submission_value")]
    fn max_submission_value(&self) -> SingleValueMapper<Self::Storage, Self::BigUint>;

    #[storage_mapper("reporting_round_id")]
    fn reporting_round_id(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[storage_mapper("latest_round_id")]
    fn latest_round_id(&self) -> SingleValueMapper<Self::Storage, u64>;

    #[storage_mapper("oracles")]
    fn oracles(&self) -> MapMapper<Self::Storage, Address, OracleStatus<Self::BigUint>>;

    #[storage_mapper("rounds")]
    fn rounds(&self) -> MapMapper<Self::Storage, u64, Round<Self::BigUint>>;

    #[storage_mapper("details")]
    fn details(&self) -> MapMapper<Self::Storage, u64, RoundDetails<Self::BigUint>>;

    #[storage_mapper("requesters")]
    fn requesters(&self) -> MapMapper<Self::Storage, Address, Requester>;

    #[storage_mapper("recorded_funds")]
    fn recorded_funds(&self) -> SingleValueMapper<Self::Storage, Funds<Self::BigUint>>;

    #[storage_mapper("deposits")]
    fn deposits(&self) -> MapMapper<Self::Storage, Address, Self::BigUint>;

    #[storage_mapper("decimals")]
    fn decimals(&self) -> SingleValueMapper<Self::Storage, u8>;

    #[storage_mapper("description")]
    fn description(&self) -> SingleValueMapper<Self::Storage, BoxedBytes>;

    #[storage_mapper("values_count")]
    fn values_count(&self) -> SingleValueMapper<Self::Storage, usize>;

    #[init]
    fn init(
        &self,
        token_id: TokenIdentifier,
        payment_amount: Self::BigUint,
        timeout: u64,
        min_submission_value: Self::BigUint,
        max_submission_value: Self::BigUint,
        decimals: u8,
        description: BoxedBytes,
        values_count: usize,
    ) -> SCResult<()> {
        self.token_id().set(&token_id);
        self.recorded_funds().set(&Funds {
            available: Self::BigUint::zero(),
            allocated: Self::BigUint::zero(),
        });

        self.update_future_rounds_internal(payment_amount, 0, 0, 0, timeout)?;
        self.min_submission_value().set(&min_submission_value);
        self.max_submission_value().set(&max_submission_value);
        self.decimals().set(&decimals);
        self.description().set(&description);
        self.values_count().set(&values_count);
        self.initialize_new_round(&0)?;
        Ok(())
    }

    #[endpoint(addFunds)]
    #[payable("*")]
    fn add_funds(
        &self,
        #[payment] payment: Self::BigUint,
        #[payment_token] token: TokenIdentifier,
    ) -> SCResult<()> {
        require!(token == self.token_id().get(), "Wrong token type");
        self.recorded_funds()
            .update(|recorded_funds| recorded_funds.available += &payment);
        let caller = &self.blockchain().get_caller();
        let deposit = self.get_deposit(caller) + payment;
        self.set_deposit(caller, &deposit);
        Ok(())
    }

    fn get_deposit(&self, address: &Address) -> Self::BigUint {
        self.deposits().get(address).unwrap_or_else(|| 0u32.into())
    }

    fn set_deposit(&self, address: &Address, amount: &Self::BigUint) {
        if amount == &Self::BigUint::zero() {
            self.deposits().remove(address);
        } else {
            self.deposits().insert(address.clone(), amount.clone());
        }
    }

    fn validate_submission_limits(&self, submission_values: &Vec<Self::BigUint>) -> SCResult<()> {
        for value in submission_values.iter() {
            require!(
                value >= &self.min_submission_value().get(),
                "value below min_submission_value"
            );
            require!(
                value <= &self.max_submission_value().get(),
                "value above max_submission_value"
            );
        }
        Ok(())
    }

    #[endpoint(submit)]
    fn submit(
        &self,
        round_id: u64,
        #[var_args] submission_values: VarArgs<Self::BigUint>,
    ) -> SCResult<()> {
        require!(
            submission_values.len() == self.values_count().get(),
            "incorrect number of values in submission"
        );
        self.validate_oracle_round(&self.blockchain().get_caller(), &round_id)?;
        let values = submission_values.into_vec();
        self.validate_submission_limits(&values)?;
        self.oracle_initialize_new_round(round_id)?;
        self.record_submission(Submission { values }, round_id)?;
        self.update_round_answer(round_id)?;
        self.pay_oracle(round_id)?;
        self.delete_round_details(round_id);
        Ok(())
    }

    #[endpoint(changeOracles)]
    fn change_oracles(
        &self,
        removed: Vec<Address>,
        added: Vec<Address>,
        added_admins: Vec<Address>,
        min_submissions: u64,
        max_submissions: u64,
        restart_delay: u64,
    ) -> SCResult<()> {
        only_owner!(self, "Only owner may call this function!");

        for oracle in removed.iter() {
            self.oracles().remove(oracle);
        }

        require!(
            added.len() == added_admins.len(),
            "need same oracle and admin count"
        );

        for (added_oracle, added_admin) in added.iter().zip(added_admins.iter()) {
            self.add_oracle(added_oracle, added_admin)?;
        }

        self.update_future_rounds_internal(
            self.payment_amount().get(),
            min_submissions,
            max_submissions,
            restart_delay,
            self.timeout().get(),
        )?;
        Ok(())
    }

    #[endpoint(updateFutureRounds)]
    fn update_future_rounds(
        &self,
        payment_amount: Self::BigUint,
        min_submissions: u64,
        max_submissions: u64,
        restart_delay: u64,
        timeout: u64,
    ) -> SCResult<()> {
        only_owner!(self, "Only owner may call this function!");
        self.update_future_rounds_internal(
            payment_amount,
            min_submissions,
            max_submissions,
            restart_delay,
            timeout,
        )
    }

    fn update_future_rounds_internal(
        &self,
        payment_amount: Self::BigUint,
        min_submissions: u64,
        max_submissions: u64,
        restart_delay: u64,
        timeout: u64,
    ) -> SCResult<()> {
        let oracle_count = self.oracle_count();
        require!(
            max_submissions >= min_submissions,
            "max must equal/exceed min"
        );
        require!(max_submissions <= oracle_count, "max cannot exceed total");
        require!(
            oracle_count == 0 || restart_delay < oracle_count,
            "delay cannot exceed total"
        );

        let recorded_funds = self.recorded_funds().get();

        require!(
            recorded_funds.available >= self.required_reserve(&payment_amount),
            "insufficient funds for payment"
        );

        if oracle_count > 0 {
            require!(min_submissions > 0, "min must be greater than 0");
        }
        self.payment_amount().set(&payment_amount);
        self.min_submission_count().set(&min_submissions);
        self.max_submission_count().set(&max_submissions);
        self.restart_delay().set(&restart_delay);
        self.timeout().set(&timeout);
        Ok(())
    }

    #[view(allocatedFunds)]
    fn allocated_funds(&self) -> Self::BigUint {
        self.recorded_funds().get().allocated
    }

    #[view(availableFunds)]
    fn available_funds(&self) -> Self::BigUint {
        self.recorded_funds().get().available
    }

    #[view(oracleCount)]
    fn oracle_count(&self) -> u64 {
        self.oracles().len() as u64
    }

    #[view(getRoundData)]
    fn get_round_data(&self, round_id: u64) -> OptionalResult<Round<Self::BigUint>> {
        self.rounds().get(&round_id).into()
    }

    #[view(latestRoundData)]
    fn latest_round_data(&self) -> OptionalResult<Round<Self::BigUint>> {
        self.get_round_data(self.latest_round_id().get())
    }

    #[view(withdrawablePayment)]
    fn withdrawable_payment(&self, oracle: Address) -> SCResult<Self::BigUint> {
        Ok(self.get_oracle_status_result(&oracle)?.withdrawable)
    }

    #[endpoint(withdrawPayment)]
    fn withdraw_payment(
        &self,
        oracle: Address,
        recipient: Address,
        amount: Self::BigUint,
    ) -> SCResult<()> {
        let mut oracle_status = self.get_oracle_status_result(&oracle)?;
        require!(
            oracle_status.admin == self.blockchain().get_caller(),
            "only callable by admin"
        );

        require!(
            oracle_status.withdrawable >= amount,
            "insufficient withdrawable funds"
        );

        self.recorded_funds()
            .update(|recorded_funds| recorded_funds.allocated -= &amount);
        oracle_status.withdrawable -= &amount;
        self.oracles().insert(oracle, oracle_status);

        self.send()
            .direct(&recipient, &self.token_id().get(), &amount, b"");
        Ok(())
    }

    #[view(withdrawableAddedFunds)]
    fn withdrawable_added_funds(&self) -> Self::BigUint {
        self.get_deposit(&self.blockchain().get_caller())
    }

    #[endpoint(withdrawFunds)]
    fn withdraw_funds(&self, amount: Self::BigUint) -> SCResult<()> {
        let recorded_funds = self.recorded_funds().get();
        let caller = &self.blockchain().get_caller();
        let deposit = self.get_deposit(caller);
        require!(amount <= deposit, "Insufficient funds to withdraw");
        require!(
            recorded_funds.available - self.required_reserve(&self.payment_amount().get())
                >= amount,
            "insufficient reserve funds"
        );
        self.recorded_funds()
            .update(|recorded_funds| recorded_funds.available -= &amount);
        let remaining = &deposit - &amount;
        self.set_deposit(caller, &remaining);
        self.send()
            .direct(caller, &self.token_id().get(), &amount, b"withdraw");
        Ok(())
    }

    #[view(getAdmin)]
    fn get_admin(&self, oracle: Address) -> SCResult<Address> {
        Ok(self.get_oracle_status_result(&oracle)?.admin)
    }

    #[endpoint(transferAdmin)]
    fn transfer_admin(&self, oracle: Address, new_admin: Address) -> SCResult<()> {
        let mut oracle_status = self.get_oracle_status_result(&oracle)?;
        require!(
            oracle_status.admin == self.blockchain().get_caller(),
            "only callable by admin"
        );
        oracle_status.pending_admin = Some(new_admin);
        self.oracles().insert(oracle, oracle_status);
        Ok(())
    }

    #[endpoint(acceptAdmin)]
    fn accept_admin(&self, oracle: Address) -> SCResult<()> {
        let mut oracle_status = self.get_oracle_status_result(&oracle)?;
        let caller = self.blockchain().get_caller();
        require!(
            oracle_status.pending_admin == Some(caller.clone()),
            "only callable by pending admin"
        );
        oracle_status.pending_admin = None;
        oracle_status.admin = caller;
        self.oracles().insert(oracle, oracle_status);
        Ok(())
    }

    #[endpoint(requestNewRound)]
    fn request_new_round(&self) -> SCResult<u64> {
        let requester_option = self.requesters().get(&self.blockchain().get_caller());
        require!(
            requester_option.map_or_else(|| false, |requester| requester.authorized),
            "not authorized requester"
        );

        let current = self.reporting_round_id().get();
        require!(
            self.rounds()
                .get(&current)
                .map_or_else(|| false, |round| round.updated_at > 0)
                || self.timed_out(&current)?,
            "prev round must be supersedable"
        );

        let new_round_id = current + 1;
        self.requester_initialize_new_round(new_round_id)?;
        Ok(new_round_id)
    }

    #[endpoint(setRequesterPermissions)]
    fn set_requester_permissions(
        &self,
        requester: Address,
        authorized: bool,
        delay: u64,
    ) -> SCResult<()> {
        only_owner!(self, "Only owner may call this function!");
        if authorized {
            self.requesters().insert(
                requester,
                Requester {
                    authorized,
                    delay,
                    last_started_round: 0,
                },
            );
        } else {
            self.requesters().remove(&requester);
        }
        Ok(())
    }

    #[view(oracleRoundState)]
    fn oracle_round_state(
        &self,
        oracle: Address,
        queried_round_id: u64,
    ) -> SCResult<OracleRoundState<Self::BigUint>> {
        if queried_round_id == 0 {
            return self.oracle_round_state_suggest_round(&oracle);
        }
        let eligible_to_submit = self.eligible_for_specific_round(&oracle, &queried_round_id)?;
        let round = self.get_round(&queried_round_id)?;
        let details = self.get_round_details(&queried_round_id)?;
        let oracle_status = self.get_oracle_status_result(&oracle)?;
        let recorded_funds = self.recorded_funds().get();
        Ok(OracleRoundState {
            eligible_to_submit,
            round_id: queried_round_id,
            latest_submission: oracle_status.latest_submission,
            started_at: round.started_at,
            timeout: details.timeout,
            available_funds: recorded_funds.available,
            oracle_count: self.oracle_count(),
            payment_amount: if round.started_at > 0 {
                details.payment_amount
            } else {
                self.payment_amount().get()
            },
        })
    }

    fn initialize_new_round(&self, round_id: &u64) -> SCResult<()> {
        if let Some(last_round) = round_id.checked_sub(1) {
            self.update_timed_out_round_info(last_round)?;
        }

        self.reporting_round_id().set(round_id);
        self.rounds().insert(
            round_id.clone(),
            Round {
                round_id: round_id.clone(),
                answer: None,
                decimals: self.decimals().get(),
                description: self.description().get(),
                started_at: self.blockchain().get_block_timestamp(),
                updated_at: self.blockchain().get_block_timestamp(),
                answered_in_round: 0,
            },
        );
        self.details().insert(
            round_id.clone(),
            RoundDetails {
                submissions: Vec::new(),
                max_submissions: self.max_submission_count().get(),
                min_submissions: self.min_submission_count().get(),
                timeout: self.timeout().get(),
                payment_amount: self.payment_amount().get(),
            },
        );
        Ok(())
    }

    fn oracle_initialize_new_round(&self, round_id: u64) -> SCResult<()> {
        if !self.new_round(&round_id) {
            return Ok(());
        }
        let oracle = self.blockchain().get_caller();
        let mut oracle_status = self.get_oracle_status_result(&oracle)?;
        let restart_delay = self.restart_delay().get();
        if round_id <= oracle_status.last_started_round + restart_delay
            && oracle_status.last_started_round != 0
        {
            return Ok(());
        }

        self.initialize_new_round(&round_id)?;

        oracle_status.last_started_round = round_id;
        self.oracles().insert(oracle, oracle_status);
        Ok(())
    }

    fn requester_initialize_new_round(&self, round_id: u64) -> SCResult<()> {
        let requester_address = self.blockchain().get_caller();
        let mut requester = self.get_requester(&requester_address)?;

        if !self.new_round(&round_id) {
            return Ok(());
        }

        require!(
            round_id > requester.last_started_round + requester.delay
                || requester.last_started_round == 0,
            "must delay requests"
        );

        self.initialize_new_round(&round_id)?;

        requester.last_started_round = round_id;
        self.requesters().insert(requester_address, requester);
        Ok(())
    }

    fn update_timed_out_round_info(&self, round_id: u64) -> SCResult<()> {
        if !self.timed_out(&round_id)? {
            return Ok(());
        }
        let mut round = self.get_round(&round_id)?;
        if let Some(prev_id) = round_id.checked_sub(1) {
            let prev_round = self.get_round(&prev_id)?;
            round.answer = prev_round.answer;
            round.answered_in_round = prev_round.answered_in_round;
        } else {
            round.answer = None;
            round.answered_in_round = 0;
        }
        round.updated_at = self.blockchain().get_block_timestamp();
        self.rounds().insert(round_id, round);
        self.details().remove(&round_id);
        Ok(())
    }

    fn eligible_for_specific_round(
        &self,
        oracle: &Address,
        queried_round_id: &u64,
    ) -> SCResult<bool> {
        if self
            .rounds()
            .get(queried_round_id)
            .map_or_else(|| false, |round| round.started_at > 0)
        {
            Ok(self.accepting_submissions(&queried_round_id)?
                && self.validate_oracle_round(oracle, queried_round_id).is_ok())
        } else {
            Ok(self.delayed(oracle, queried_round_id)?
                && self.validate_oracle_round(oracle, queried_round_id).is_ok())
        }
    }

    fn oracle_round_state_suggest_round(
        &self,
        oracle: &Address,
    ) -> SCResult<OracleRoundState<Self::BigUint>> {
        let oracle_status = self.get_oracle_status_result(oracle)?;

        let reporting_round_id = self.reporting_round_id().get();
        let should_supersede = oracle_status.last_reported_round == reporting_round_id
            || !self.accepting_submissions(&reporting_round_id)?;
        // Instead of nudging oracles to submit to the next round, the inclusion of
        // the should_supersede bool in the if condition pushes them towards
        // submitting in a currently open round.
        let mut eligible_to_submit: bool;
        let round: Round<Self::BigUint>;
        let round_id: u64;
        let payment_amount: Self::BigUint;
        if self.supersedable(&reporting_round_id)? && should_supersede {
            round_id = reporting_round_id + 1;
            round = self.get_round(&round_id)?;

            payment_amount = self.payment_amount().get();
            eligible_to_submit = self.delayed(&oracle, &round_id)?;
        } else {
            round_id = reporting_round_id;
            round = self.get_round(&round_id)?;

            let round_details = self.get_round_details(&round_id)?;
            payment_amount = round_details.payment_amount;
            eligible_to_submit = self.accepting_submissions(&round_id)?;
        }

        if self.validate_oracle_round(&oracle, &round_id).is_err() {
            eligible_to_submit = false;
        }

        let recorded_funds = self.recorded_funds().get();
        let round_details = self.get_round_details(&round_id)?;

        Ok(OracleRoundState {
            eligible_to_submit,
            round_id,
            latest_submission: oracle_status.latest_submission,
            started_at: round.started_at,
            timeout: round_details.timeout,
            available_funds: recorded_funds.available,
            oracle_count: self.oracle_count(),
            payment_amount,
        })
    }

    fn update_round_answer(&self, round_id: u64) -> SCResult<()> {
        let details = self.get_round_details(&round_id)?;
        if (details.submissions.len() as u64) < details.min_submissions {
            return Ok(());
        }

        match median::calculate_submission_median(details.submissions) {
            Result::Ok(new_answer) => {
                let mut round = self.get_round(&round_id)?;
                round.answer = new_answer;
                round.updated_at = self.blockchain().get_block_timestamp();
                round.answered_in_round = round_id;
                self.rounds().insert(round_id, round);
                self.latest_round_id().set(&round_id);
                Ok(())
            }
            Result::Err(error_message) => SCResult::Err(error_message.into()),
        }
    }

    fn subtract_amount_from_deposits(&self, amount: &Self::BigUint) {
        let mut remaining = amount.clone();
        let mut final_amounts: Vec<(Address, Self::BigUint)> = Vec::new();
        for (account, deposit) in self.deposits().iter() {
            if remaining == Self::BigUint::zero() {
                break;
            }
            if deposit <= remaining {
                final_amounts.push((account, Self::BigUint::zero()));
                remaining -= deposit;
            } else {
                final_amounts.push((account, deposit - remaining));
                remaining = Self::BigUint::zero();
            }
        }
        for (account, final_amount) in final_amounts.iter() {
            self.set_deposit(account, final_amount);
        }
    }

    fn pay_oracle(&self, round_id: u64) -> SCResult<()> {
        let round_details = self.get_round_details(&round_id)?;
        let oracle = self.blockchain().get_caller();
        let mut oracle_status = self.get_oracle_status_result(&oracle)?;

        let payment = round_details.payment_amount;
        self.recorded_funds().update(|recorded_funds| {
            recorded_funds.available -= &payment;
            recorded_funds.allocated += &payment;
        });
        self.subtract_amount_from_deposits(&payment);

        oracle_status.withdrawable += &payment;
        self.oracles().insert(oracle, oracle_status);
        Ok(())
    }

    fn record_submission(
        &self,
        submission: Submission<Self::BigUint>,
        round_id: u64,
    ) -> SCResult<()> {
        require!(
            self.accepting_submissions(&round_id)?,
            "round not accepting submissions"
        );

        let mut round_details = self.get_round_details(&round_id)?;
        let oracle = self.blockchain().get_caller();
        let mut oracle_status = self.get_oracle_status_result(&oracle)?;
        round_details.submissions.push(submission.clone());
        oracle_status.last_reported_round = round_id;
        oracle_status.latest_submission = Some(submission);
        self.details().insert(round_id, round_details);
        self.oracles().insert(oracle, oracle_status);
        Ok(())
    }

    fn delete_round_details(&self, round_id: u64) {
        if let Some(details) = self.details().get(&round_id) {
            if (details.submissions.len() as u64) < details.max_submissions {
                return;
            }
        }
        self.details().remove(&round_id);
    }

    fn timed_out(&self, round_id: &u64) -> SCResult<bool> {
        let round = self.get_round(round_id)?;
        let started_at = round.started_at;
        let details = self.get_round_details(round_id)?;
        let round_timeout = details.timeout;
        Ok(round_id == &0
            || (started_at > 0
                && round_timeout > 0
                && started_at + round_timeout < self.blockchain().get_block_timestamp()))
    }

    fn get_starting_round(&self, oracle: &Address) -> u64 {
        let current_round = self.reporting_round_id().get();
        if current_round != 0 {
            if let Some(oracle_status) = self.get_oracle_status_option(&oracle) {
                if current_round == oracle_status.ending_round {
                    return current_round;
                }
            }
        }
        current_round + 1
    }

    fn previous_and_current_unanswered(&self, round_id: u64, rr_id: u64) -> SCResult<bool> {
        let round = self.get_round(&rr_id)?;
        Ok(round_id + 1 == rr_id && round.updated_at == 0)
    }

    #[view(requiredReserve)]
    fn required_reserve(&self, payment: &Self::BigUint) -> Self::BigUint {
        payment * &Self::BigUint::from(self.oracle_count()) * Self::BigUint::from(RESERVE_ROUNDS)
    }

    fn add_oracle(&self, oracle: &Address, admin: &Address) -> SCResult<()> {
        require!(!self.oracle_enabled(oracle), "oracle already enabled");

        self.oracles().insert(
            oracle.clone(),
            OracleStatus {
                withdrawable: Self::BigUint::zero(),
                starting_round: self.get_starting_round(oracle),
                ending_round: ROUND_MAX,
                last_reported_round: 0,
                last_started_round: 0,
                latest_submission: None,
                admin: admin.clone(),
                pending_admin: None,
            },
        );
        Ok(())
    }

    fn validate_oracle_round(&self, oracle: &Address, round_id: &u64) -> SCResult<()> {
        let oracle_status = self.get_oracle_status_result(&oracle)?;
        let reporting_round_id = self.reporting_round_id().get();

        require!(oracle_status.starting_round != 0, "not enabled oracle");
        require!(
            oracle_status.starting_round <= *round_id,
            "not yet enabled oracle"
        );
        require!(
            oracle_status.ending_round >= *round_id,
            "no longer allowed oracle"
        );
        require!(
            oracle_status.last_reported_round < *round_id,
            "cannot report on previous rounds"
        );
        require!(
            *round_id == reporting_round_id
                || *round_id == reporting_round_id + 1
                || self.previous_and_current_unanswered(*round_id, reporting_round_id)?,
            "invalid round to report"
        );
        require!(
            *round_id == 1 || self.supersedable(&(*round_id - 1))?,
            "previous round not supersedable"
        );
        Ok(())
    }

    fn supersedable(&self, round_id: &u64) -> SCResult<bool> {
        let round = self.get_round(round_id)?;
        let timed_out = self.timed_out(round_id)?;
        Ok(round.updated_at > 0 || timed_out)
    }

    fn oracle_enabled(&self, oracle: &Address) -> bool {
        self.oracles().contains_key(oracle)
    }

    fn accepting_submissions(&self, round_id: &u64) -> SCResult<bool> {
        let details = self.get_round_details(round_id)?;
        Ok(details.max_submissions != 0)
    }

    fn delayed(&self, oracle: &Address, round_id: &u64) -> SCResult<bool> {
        let oracle_status = self.get_oracle_status_result(oracle)?;
        let last_started = oracle_status.last_started_round;
        Ok(*round_id > last_started + self.restart_delay().get() || last_started == 0)
    }

    fn new_round(&self, round_id: &u64) -> bool {
        *round_id == self.reporting_round_id().get() + 1
    }

    fn get_oracle_status_option(&self, oracle: &Address) -> Option<OracleStatus<Self::BigUint>> {
        self.oracles().get(oracle)
    }

    fn get_oracle_status_result(&self, oracle: &Address) -> SCResult<OracleStatus<Self::BigUint>> {
        if let Some(oracle_status) = self.oracles().get(oracle) {
            return Ok(oracle_status);
        }
        sc_error!("No oracle at given address")
    }

    fn get_round(&self, round_id: &u64) -> SCResult<Round<Self::BigUint>> {
        if let Some(round) = self.rounds().get(round_id) {
            return Ok(round);
        }
        sc_error!("No round for given round id")
    }

    fn get_round_details(&self, round_id: &u64) -> SCResult<RoundDetails<Self::BigUint>> {
        if let Some(round_details) = self.details().get(round_id) {
            return Ok(round_details);
        }
        sc_error!("No round details for given round id")
    }

    fn get_requester(&self, requester_address: &Address) -> SCResult<Requester> {
        if let Some(requester) = self.requesters().get(requester_address) {
            return Ok(requester);
        }
        sc_error!("No requester has the given address")
    }

    #[view(getOracles)]
    fn get_oracles(&self) -> MultiResultVec<Address> {
        self.oracles().keys().collect()
    }
}
