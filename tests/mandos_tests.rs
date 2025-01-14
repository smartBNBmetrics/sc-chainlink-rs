use elrond_wasm::*;
use elrond_wasm_debug::*;

fn contract_map() -> ContractMap<TxContext> {
    let mut contract_map = ContractMap::new();
    contract_map.register_contract(
        "file:../client/output/client.wasm",
        Box::new(|context| Box::new(client::contract_obj(context))),
    );
    contract_map.register_contract(
        "file:../oracle/output/oracle.wasm",
        Box::new(|context| Box::new(oracle::contract_obj(context))),
    );
    contract_map.register_contract(
        "file:../aggregator/output/aggregator.wasm",
        Box::new(|context| Box::new(aggregator::contract_obj(context))),
    );
    contract_map.register_contract(
        "file:../price-aggregator/output/price-aggregator.wasm",
        Box::new(|context| Box::new(price_aggregator::contract_obj(context))),
    );
    contract_map
}

#[test]
fn init() {
    elrond_wasm_debug::mandos_rs("mandos/init.scen.json", &contract_map());
}

#[test]
fn client_request() {
    elrond_wasm_debug::mandos_rs("mandos/client-request.scen.json", &contract_map());
}

#[test]
fn aggregator() {
    elrond_wasm_debug::mandos_rs("mandos/aggregator.scen.json", &contract_map());
}

#[test]
fn init_price_aggregator() {
    elrond_wasm_debug::mandos_rs("mandos/init-price-aggregator.scen.json", &contract_map());
}

#[test]
fn price_aggregator() {
    elrond_wasm_debug::mandos_rs("mandos/price-aggregator.scen.json", &contract_map());
}

#[test]
fn price_aggregator_balance() {
    elrond_wasm_debug::mandos_rs("mandos/price-aggregator-balance.scen.json", &contract_map());
}
