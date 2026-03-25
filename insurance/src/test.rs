use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register_contract(None, Insurance);
    let owner = Address::generate(&env);
    (env, id, owner)
}

#[test]
fn create_policy_and_get_policy() {
    let (env, id, owner) = setup();
    let client = InsuranceClient::new(&env, &id);

    let pid = client.create_policy(
        &owner,
        &String::from_str(&env, "Health"),
        &String::from_str(&env, "health"),
        &100,
        &10_000,
    );

    assert_eq!(pid, 1);
    let p = client.get_policy(&pid).unwrap();
    assert_eq!(p.owner, owner);
    assert_eq!(p.monthly_premium, 100);
    assert!(p.active);
}

#[test]
fn create_policy_rejects_invalid_amounts() {
    let (env, id, owner) = setup();
    let client = InsuranceClient::new(&env, &id);

    let r1 = client.try_create_policy(
        &owner,
        &String::from_str(&env, "Bad"),
        &String::from_str(&env, "life"),
        &0,
        &10,
    );
    assert_eq!(r1, Err(Ok(InsuranceError::InvalidAmount)));

    let r2 = client.try_create_policy(
        &owner,
        &String::from_str(&env, "Bad2"),
        &String::from_str(&env, "life"),
        &10,
        &0,
    );
    assert_eq!(r2, Err(Ok(InsuranceError::InvalidAmount)));
}

#[test]
fn pay_premium_updates_next_payment_date() {
    let (env, id, owner) = setup();
    let client = InsuranceClient::new(&env, &id);

    let pid = client.create_policy(
        &owner,
        &String::from_str(&env, "Policy"),
        &String::from_str(&env, "auto"),
        &100,
        &10_000,
    );

    let before = client.get_policy(&pid).unwrap().next_payment_date;
    env.ledger().set_timestamp(env.ledger().timestamp() + 500);
    client.pay_premium(&owner, &pid);
    let after = client.get_policy(&pid).unwrap().next_payment_date;
    assert!(after > before);
}

#[test]
fn deactivate_policy_blocks_premium_payment() {
    let (env, id, owner) = setup();
    let client = InsuranceClient::new(&env, &id);

    let pid = client.create_policy(
        &owner,
        &String::from_str(&env, "Policy"),
        &String::from_str(&env, "property"),
        &100,
        &10_000,
    );

    let ok = client.deactivate_policy(&owner, &pid);
    assert!(ok);

    let result = client.try_pay_premium(&owner, &pid);
    assert_eq!(result, Err(Ok(InsuranceError::PolicyInactive)));

    let p = client.get_policy(&pid).unwrap();
    assert!(!p.active);
}

#[test]
fn active_policies_are_paginated_and_filtered() {
    let (env, id, owner) = setup();
    let client = InsuranceClient::new(&env, &id);

    let id1 = client.create_policy(
        &owner,
        &String::from_str(&env, "P1"),
        &String::from_str(&env, "health"),
        &100,
        &10_000,
    );
    let id2 = client.create_policy(
        &owner,
        &String::from_str(&env, "P2"),
        &String::from_str(&env, "life"),
        &200,
        &20_000,
    );
    let _id3 = client.create_policy(
        &owner,
        &String::from_str(&env, "P3"),
        &String::from_str(&env, "auto"),
        &300,
        &30_000,
    );

    client.deactivate_policy(&owner, &id2);

    let page = client.get_active_policies(&owner, &0, &10);
    assert_eq!(page.count, 2);
    for p in page.items.iter() {
        assert!(p.active);
        assert_eq!(p.owner, owner);
    }

    let all_page = client.get_all_policies_for_owner(&owner, &0, &10);
    assert_eq!(all_page.count, 3);
    assert_eq!(id1, 1);
}

#[test]
fn monthly_premium_total_counts_only_active() {
    let (env, id, owner) = setup();
    let client = InsuranceClient::new(&env, &id);

    let p1 = client.create_policy(
        &owner,
        &String::from_str(&env, "P1"),
        &String::from_str(&env, "health"),
        &100,
        &10_000,
    );
    client.create_policy(
        &owner,
        &String::from_str(&env, "P2"),
        &String::from_str(&env, "life"),
        &200,
        &20_000,
    );

    assert_eq!(client.get_total_monthly_premium(&owner), 300);
    client.deactivate_policy(&owner, &p1);
    assert_eq!(client.get_total_monthly_premium(&owner), 200);
}

#[test]
fn schedule_lifecycle_and_execution() {
    let (env, id, owner) = setup();
    let client = InsuranceClient::new(&env, &id);

    let pid = client.create_policy(
        &owner,
        &String::from_str(&env, "Sched"),
        &String::from_str(&env, "health"),
        &100,
        &10_000,
    );

    let now = env.ledger().timestamp();
    let sid = client.create_premium_schedule(&owner, &pid, &(now + 1000), &2592000);

    let sched = client.get_premium_schedule(&sid).unwrap();
    assert!(sched.active);

    let modified = client.modify_premium_schedule(&owner, &sid, &(now + 1500), &2_678_400);
    assert!(modified);

    env.ledger().set_timestamp(now + 2000);
    let executed = client.execute_due_premium_schedules();
    assert_eq!(executed.len(), 1);
    assert_eq!(executed.get(0).unwrap(), sid);

    let cancelled = client.cancel_premium_schedule(&owner, &sid);
    assert!(cancelled);
    let sched2 = client.get_premium_schedule(&sid).unwrap();
    assert!(!sched2.active);
}
