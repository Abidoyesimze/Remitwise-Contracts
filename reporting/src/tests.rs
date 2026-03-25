// tests module is compiled only when running tests via cfg(test) at lib.rs
use crate::*;
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env, String, Vec};

mod remittance_split_mock {
    use super::*;

    #[contract]
    pub struct RemittanceSplitMock;

    #[contractimpl]
    impl RemittanceSplitTrait for RemittanceSplitMock {
        fn get_split(env: &Env) -> Vec<u32> {
            let mut out = Vec::new(env);
            out.push_back(50);
            out.push_back(30);
            out.push_back(15);
            out.push_back(5);
            out
        }

        fn calculate_split(env: Env, total_amount: i128) -> Vec<i128> {
            let mut out = Vec::new(&env);
            out.push_back(total_amount * 50 / 100);
            out.push_back(total_amount * 30 / 100);
            out.push_back(total_amount * 15 / 100);
            out.push_back(total_amount * 5 / 100);
            out
        }
    }
}

mod savings_mock {
    use super::*;

    #[contract]
    pub struct SavingsGoalsMock;

    #[contractimpl]
    impl SavingsGoalsTrait for SavingsGoalsMock {
        fn get_all_goals(env: Env, owner: Address) -> Vec<SavingsGoal> {
            let mut goals = Vec::new(&env);
            goals.push_back(SavingsGoal {
                id: 1,
                owner,
                name: String::from_str(&env, "Emergency"),
                target_amount: 1000,
                current_amount: 500,
                target_date: 2_000_000_000,
                locked: true,
                unlock_date: None,
            });
            goals
        }

        fn is_goal_completed(_env: Env, _goal_id: u32) -> bool {
            false
        }
    }
}

mod bills_mock {
    use super::*;

    #[contract]
    pub struct BillPaymentsMock;

    #[contractimpl]
    impl BillPaymentsTrait for BillPaymentsMock {
        fn get_unpaid_bills(env: Env, owner: Address, _cursor: u32, _limit: u32) -> BillPage {
            let mut items = Vec::new(&env);
            items.push_back(Bill {
                id: 1,
                owner,
                name: String::from_str(&env, "Power"),
                amount: 100,
                due_date: 2_000_000_000,
                recurring: false,
                frequency_days: 0,
                paid: false,
                created_at: 1_700_000_000,
                paid_at: None,
                schedule_id: None,
                currency: String::from_str(&env, "XLM"),
            });
            BillPage {
                items: items.clone(),
                next_cursor: 0,
                count: items.len(),
            }
        }

        fn get_total_unpaid(_env: Env, _owner: Address) -> i128 {
            100
        }

        fn get_all_bills_for_owner(
            env: Env,
            owner: Address,
            _cursor: u32,
            _limit: u32,
        ) -> BillPage {
            Self::get_unpaid_bills(env, owner, 0, 50)
        }
    }
}

mod insurance_mock {
    use super::*;

    #[contract]
    pub struct InsuranceMock;

    #[contractimpl]
    impl InsuranceTrait for InsuranceMock {
        fn get_active_policies(env: Env, owner: Address, _cursor: u32, _limit: u32) -> PolicyPage {
            let mut items = Vec::new(&env);
            items.push_back(InsurancePolicy {
                id: 1,
                owner,
                name: String::from_str(&env, "Health"),
                coverage_type: String::from_str(&env, "health"),
                monthly_premium: 50,
                coverage_amount: 10_000,
                active: true,
                next_payment_date: 2_000_000_000,
                schedule_id: None,
            });
            PolicyPage {
                items: items.clone(),
                next_cursor: 0,
                count: items.len(),
            }
        }

        fn get_total_monthly_premium(_env: Env, _owner: Address) -> i128 {
            50
        }
    }
}

fn setup_reporting() -> (Env, ReportingContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let reporting_id = env.register_contract(None, ReportingContract);
    let client = ReportingContractClient::new(&env, &reporting_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.init(&admin);

    let rem_id = env.register_contract(None, remittance_split_mock::RemittanceSplitMock);
    let sav_id = env.register_contract(None, savings_mock::SavingsGoalsMock);
    let bill_id = env.register_contract(None, bills_mock::BillPaymentsMock);
    let ins_id = env.register_contract(None, insurance_mock::InsuranceMock);
    let fam = Address::generate(&env);

    client.configure_addresses(&admin, &rem_id, &sav_id, &bill_id, &ins_id, &fam);

    // SAFETY: tests keep env and client alive together.
    let client: ReportingContractClient<'static> = unsafe { core::mem::transmute(client) };
    (env, client, admin, user)
}

#[test]
fn init_and_configure_addresses() {
    let (env, client, admin, _) = setup_reporting();
    assert_eq!(client.get_admin().unwrap(), admin);
    assert!(client.get_addresses().is_some());
    let _ = env;
}

#[test]
fn generates_health_report_and_store_roundtrip() {
    let (env, client, _admin, user) = setup_reporting();

    let report = client.get_financial_health_report(&user, &1_000, &1_700_000_000, &1_800_000_000);
    assert!(report.health_score.score > 0);
    assert_eq!(report.remittance_summary.total_received, 1_000);

    let key = 202601u64;
    assert!(client.store_report(&user, &report, &key));
    let stored = client.get_stored_report(&user, &key).unwrap();
    assert_eq!(stored.generated_at, report.generated_at);
    let _ = env;
}
