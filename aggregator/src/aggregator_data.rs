imports!();
derive_imports!();

#[derive(NestedEncode, NestedDecode, TopEncode, TopDecode, TypeAbi)]
pub struct Round {
    pub answer: u64,
    pub started_at: u64,
    pub updated_at: u64,
    pub answered_in_round: u64,
}

#[derive(NestedEncode, NestedDecode, TopEncode, TopDecode, TypeAbi)]
pub struct RoundDetails<BigUint: BigUintApi> {
    pub submissions: Vec<u64>,
    pub max_submissions: u64,
    pub min_submissions: u64,
    pub timeout: u64,
    pub payment_amount: BigUint,
}

#[derive(NestedEncode, NestedDecode, TopEncode, TopDecode, TypeAbi)]
pub struct OracleStatus<BigUint: BigUintApi> {
    pub withdrawable: BigUint,
    pub starting_round: u64,
    pub ending_round: u64,
    pub last_reported_round: u64,
    pub last_started_round: u64,
    pub latest_submission: u64,
    pub admin: Address,
    pub pending_admin: Option<Address>,
}

#[derive(NestedEncode, NestedDecode, TopEncode, TopDecode, TypeAbi)]
pub struct Requester {
    pub authorized: bool,
    pub delay: u64,
    pub last_started_round: u64,
}

#[derive(NestedEncode, NestedDecode, TopEncode, TopDecode, TypeAbi)]
pub struct Funds<BigUint: BigUintApi> {
    pub available: BigUint,
    pub allocated: BigUint,
}

#[derive(NestedEncode, NestedDecode, TopEncode, TopDecode, TypeAbi)]
pub struct OracleRoundState<BigUint: BigUintApi> {
    pub eligible_to_submit: bool,
    pub round_id: u64,
    pub latest_submission: u64,
    pub started_at: u64,
    pub timeout: u64,
    pub available_funds: BigUint,
    pub oracle_count: u64,
    pub payment_amount: BigUint,
}
