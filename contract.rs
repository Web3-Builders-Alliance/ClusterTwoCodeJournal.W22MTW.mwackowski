/*
imports - the one that looks interesting is State structure, that consists of, amongst others, two vectors:
collateral and counter_offer
*/

use cosmwasm_std::{
    entry_point, to_binary, BankMsg, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
};

use crate::error::ContractError;
use crate::msg::{ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{State, CONFIG};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    /* instantiate message consists of 2 attributes:
    - pub counter_offer: Vec<Coin>,
    - pub expires: u64,
    below condition validates if 'expires' attribute is not lower than current block height 
    - if so, it will return an error
    */
    if msg.expires <= env.block.height {
        return Err(ContractError::OptionExpired {
            expired: msg.expires,
        });
    }

    /*
    state declaration - both creator and owner are set as sender, collateral of option set to info.funds,
    counter_offer as an attribute of Instantiate message and expires that has been validated above
    */
    let state = State {
        creator: info.sender.clone(),
        owner: info.sender.clone(),
        collateral: info.funds,
        counter_offer: msg.counter_offer,
        expires: msg.expires,
    };

    /* save new state to storage */
    CONFIG.save(deps.storage, &state)?;

    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    /*
    three execute message enums matched and their associated functions
    */
    match msg {
        ExecuteMsg::Transfer { recipient } => execute_transfer(deps, env, info, recipient),
        ExecuteMsg::Execute {} => execute_execute(deps, env, info),
        ExecuteMsg::Burn {} => execute_burn(deps, env, info),
    }
}

/*
ExecuteMsg::Transfer associated function - accepts 4 params, of which recipient is an extra one
It firstly validates if sender of the message is the state.owner (instantiated one), if not - raises error
Then it valides if recipient value is an appropriate address and sets it as a new state.owner to storage
*/
pub fn execute_transfer(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    recipient: String,
) -> Result<Response, ContractError> {
    // ensure msg sender is the owner
    let mut state = CONFIG.load(deps.storage)?;
    if info.sender != state.owner {
        return Err(ContractError::Unauthorized {});
    }

    // set new owner on state
    state.owner = deps.api.addr_validate(&recipient)?;
    CONFIG.save(deps.storage, &state)?;

    let res =
        Response::new().add_attributes([("action", "transfer"), ("owner", recipient.as_str())]);
    Ok(res)
}

/*
ExecuteMsg::Execute associated function:
- loads the state from the storage
- valides if the sender of the message is equal to the owner of the state, which is either the sender
of InstantiateMsg or the owner transferred through ExecuteMsg::Transfer
- checks if the option has not expired yet
- checks if the funds sent through the message are equal to state.counter_offer attribute
- if above conditions are met, the counter offer is sent to the creator of the option (InstantiateMsg)
- collateral is sent to the owner of the state (which is either the sender of InstantiateMsg 
    or the owner transferred through ExecuteMsg::Transfer)
- removes the option from the storage
*/
pub fn execute_execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    // ensure msg sender is the owner
    let state = CONFIG.load(deps.storage)?;
    if info.sender != state.owner {
        return Err(ContractError::Unauthorized {});
    }

    // ensure not expired
    if env.block.height >= state.expires {
        return Err(ContractError::OptionExpired {
            expired: state.expires,
        });
    }

    // ensure sending proper counter_offer
    if info.funds != state.counter_offer {
        return Err(ContractError::CounterOfferMismatch {
            offer: info.funds,
            counter_offer: state.counter_offer,
        });
    }

    // release counter_offer to creator
    let mut res = Response::new();
    res = res.add_message(BankMsg::Send {
        to_address: state.creator.to_string(),
        amount: state.counter_offer,
    });

    // release collateral to sender
    res = res.add_message(BankMsg::Send {
        to_address: state.owner.to_string(),
        amount: state.collateral,
    });

    // delete the option
    CONFIG.remove(deps.storage);

    res = res.add_attribute("action", "execute");
    Ok(res)
}

/* ExecuteMsg::Burn associated function:
- checks if option has expired
- checks if there are no funds sents
- collateral is sent to the creator of the state
- removes option from the storage
*/
pub fn execute_burn(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    // ensure is expired
    let state = CONFIG.load(deps.storage)?;
    if env.block.height < state.expires {
        return Err(ContractError::OptionNotExpired {
            expires: state.expires,
        });
    }

    // ensure sending proper counter_offer
    if !info.funds.is_empty() {
        return Err(ContractError::FundsSentWithBurn {});
    }

    // release collateral to creator
    let mut res = Response::new();
    res = res.add_message(BankMsg::Send {
        to_address: state.creator.to_string(),
        amount: state.collateral,
    });

    // delete the option
    CONFIG.remove(deps.storage);

    res = res.add_attribute("action", "burn");
    Ok(res)
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let state = CONFIG.load(deps.storage)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{attr, coins, CosmosMsg, coin};

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            counter_offer: coins(40, "ETH"),
            expires: 100_000,
        };
        let info = mock_info("creator", &coins(1, "BTC"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the state
        let res = query_config(deps.as_ref()).unwrap();
        assert_eq!(100_000, res.expires);
        assert_eq!("creator", res.owner.as_str());
        assert_eq!("creator", res.creator.as_str());
        assert_eq!(coins(1, "BTC"), res.collateral);
        assert_eq!(coins(40, "ETH"), res.counter_offer);
    }

    #[test]
    fn transfer() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            counter_offer: coins(40, "ETH"),
            expires: 100_000,
        };
        let info = mock_info("creator", &coins(1, "BTC"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // random cannot transfer
        let info = mock_info("anyone", &[]);
        let err =
            execute_transfer(deps.as_mut(), mock_env(), info, "anyone".to_string()).unwrap_err();
        match err {
            ContractError::Unauthorized {} => {}
            e => panic!("unexpected error: {}", e),
        }

        // owner can transfer
        let info = mock_info("creator", &[]);
        let res = execute_transfer(deps.as_mut(), mock_env(), info, "someone".to_string()).unwrap();
        assert_eq!(res.attributes.len(), 2);
        assert_eq!(res.attributes[0], attr("action", "transfer"));

        // check updated properly
        let res = query_config(deps.as_ref()).unwrap();
        assert_eq!("someone", res.owner.as_str());
        assert_eq!("creator", res.creator.as_str());
    }

    #[test]
    fn execute() {
        let mut deps = mock_dependencies();

        let amount = coins(40, "ETH");
        let collateral = coins(1, "BTC");
        let expires = 100_000;
        let msg = InstantiateMsg {
            counter_offer: amount.clone(),
            expires,
        };
        let info = mock_info("creator", &collateral);

        // we can just call .unwrap() to assert this was a success
        let _ = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // set new owner
        let info = mock_info("creator", &[]);
        let _ = execute_transfer(deps.as_mut(), mock_env(), info, "owner".to_string()).unwrap();

        // random cannot execute
        let info = mock_info("creator", &amount);
        let err = execute_execute(deps.as_mut(), mock_env(), info).unwrap_err();
        match err {
            ContractError::Unauthorized {} => {}
            e => panic!("unexpected error: {}", e),
        }

        // expired cannot execute
        let info = mock_info("owner", &amount);
        let mut env = mock_env();
        env.block.height = 200_000;
        let err = execute_execute(deps.as_mut(), env, info).unwrap_err();
        match err {
            ContractError::OptionExpired { expired } => assert_eq!(expired, expires),
            e => panic!("unexpected error: {}", e),
        }

        // bad counter_offer cannot execute
        let msg_offer = coins(39, "ETH");
        let info = mock_info("owner", &msg_offer);
        let err = execute_execute(deps.as_mut(), mock_env(), info).unwrap_err();
        match err {
            ContractError::CounterOfferMismatch {
                offer,
                counter_offer,
            } => {
                assert_eq!(msg_offer, offer);
                assert_eq!(amount, counter_offer);
            }
            e => panic!("unexpected error: {}", e),
        }

        // proper execution
        let info = mock_info("owner", &amount);
        let res = execute_execute(deps.as_mut(), mock_env(), info).unwrap();
        assert_eq!(res.messages.len(), 2);
        assert_eq!(
            res.messages[0].msg,
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "creator".into(),
                amount,
            })
        );
        assert_eq!(
            res.messages[1].msg,
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "owner".into(),
                amount: collateral,
            })
        );

        // check deleted
        let _ = query_config(deps.as_ref()).unwrap_err();
    }

    #[test]
    fn burn() {
        let mut deps = mock_dependencies();

        let counter_offer = coins(40, "ETH");
        let collateral = coins(1, "BTC");
        let msg_expires = 100_000;
        let msg = InstantiateMsg {
            counter_offer: counter_offer.clone(),
            expires: msg_expires,
        };
        let info = mock_info("creator", &collateral);

        // we can just call .unwrap() to assert this was a success
        let _ = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // set new owner
        let info = mock_info("creator", &[]);
        let _ = execute_transfer(deps.as_mut(), mock_env(), info, "owner".to_string()).unwrap();

        // non-expired cannot execute
        let info = mock_info("anyone", &[]);
        let err = execute_burn(deps.as_mut(), mock_env(), info).unwrap_err();
        match err {
            ContractError::OptionNotExpired { expires } => assert_eq!(expires, msg_expires),
            e => panic!("unexpected error: {}", e),
        }

        // with funds cannot execute
        let info = mock_info("anyone", &counter_offer);
        let mut env = mock_env();
        env.block.height = 200_000;
        let err = execute_burn(deps.as_mut(), env, info).unwrap_err();
        match err {
            ContractError::FundsSentWithBurn {} => {}
            e => panic!("unexpected error: {}", e),
        }

        // expired returns funds
        let info = mock_info("anyone", &[]);
        let mut env = mock_env();
        env.block.height = 200_000;
        let res = execute_burn(deps.as_mut(), env, info).unwrap();
        assert_eq!(res.messages.len(), 1);
        assert_eq!(
            res.messages[0].msg,
            CosmosMsg::Bank(BankMsg::Send {
                to_address: "creator".into(),
                amount: collateral,
            })
        );

        // check deleted
        let _ = query_config(deps.as_ref()).unwrap_err();
    }
}
