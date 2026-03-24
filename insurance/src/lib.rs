#![no_std]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Map, String,
    Symbol, Vec,
};

const INSTANCE_LIFETIME_THRESHOLD: u32 = 17280;
const INSTANCE_BUMP_AMOUNT: u32 = 518400;
const MAX_BATCH_SIZE: u32 = 50;

pub const DEFAULT_PAGE_LIMIT: u32 = 20;
pub const MAX_PAGE_LIMIT: u32 = 50;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum InsuranceError {
    PolicyNotFound = 1,
    Unauthorized = 2,
    InvalidAmount = 3,
    PolicyInactive = 4,
    ContractPaused = 5,
    FunctionPaused = 6,
    InvalidTimestamp = 7,
    BatchTooLarge = 8,
    ScheduleNotFound = 9,
}

pub mod pause_functions {
    use soroban_sdk::{symbol_short, Symbol};
    pub const CREATE_POLICY: Symbol = symbol_short!("crt_pol");
    pub const PAY_PREMIUM: Symbol = symbol_short!("pay_prem");
    pub const DEACTIVATE: Symbol = symbol_short!("deact");
    pub const CREATE_SCHED: Symbol = symbol_short!("crt_sch");
    pub const MODIFY_SCHED: Symbol = symbol_short!("mod_sch");
    pub const CANCEL_SCHED: Symbol = symbol_short!("can_sch");
}

#[contracttype]
#[derive(Clone)]
pub struct InsurancePolicy {
    pub id: u32,
    pub owner: Address,
    pub name: String,
    pub external_ref: Option<String>,
    pub coverage_type: String,
    pub monthly_premium: i128,
    pub coverage_amount: i128,
    pub active: bool,
    pub next_payment_date: u64,
    pub schedule_id: Option<u32>,
    pub tags: Vec<String>,
}

#[contracttype]
#[derive(Clone)]
pub struct PolicyPage {
    pub items: Vec<InsurancePolicy>,
    pub next_cursor: u32,
    pub count: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct PremiumSchedule {
    pub id: u32,
    pub owner: Address,
    pub policy_id: u32,
    pub next_due: u64,
    pub interval: u64,
    pub recurring: bool,
    pub active: bool,
    pub created_at: u64,
    pub last_executed: Option<u64>,
    pub missed_count: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum InsuranceEvent {
    PolicyCreated,
    PremiumPaid,
    PolicyDeactivated,
    ExternalRefUpdated,
    ScheduleCreated,
    ScheduleExecuted,
    ScheduleMissed,
    ScheduleModified,
    ScheduleCancelled,
}

#[contract]
pub struct Insurance;

#[contractimpl]
impl Insurance {
    fn clamp_limit(limit: u32) -> u32 {
        if limit == 0 {
            DEFAULT_PAGE_LIMIT
        } else if limit > MAX_PAGE_LIMIT {
            MAX_PAGE_LIMIT
        } else {
            limit
        }
    }

    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    fn get_pause_admin(env: &Env) -> Option<Address> {
        env.storage().instance().get(&symbol_short!("PAUSE_ADM"))
    }

    fn get_global_paused(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&symbol_short!("PAUSED"))
            .unwrap_or(false)
    }

    fn is_function_paused(env: &Env, func: Symbol) -> bool {
        env.storage()
            .instance()
            .get::<_, Map<Symbol, bool>>(&symbol_short!("PAUSED_FN"))
            .unwrap_or_else(|| Map::new(env))
            .get(func)
            .unwrap_or(false)
    }

    fn require_not_paused(env: &Env, func: Symbol) -> Result<(), InsuranceError> {
        if Self::get_global_paused(env) {
            return Err(InsuranceError::ContractPaused);
        }
        if Self::is_function_paused(env, func) {
            return Err(InsuranceError::FunctionPaused);
        }
        Ok(())
    }

    pub fn set_pause_admin(
        env: Env,
        caller: Address,
        new_admin: Address,
    ) -> Result<(), InsuranceError> {
        caller.require_auth();
        let current = Self::get_pause_admin(&env);
        match current {
            None => {
                if caller != new_admin {
                    return Err(InsuranceError::Unauthorized);
                }
            }
            Some(admin) if admin != caller => return Err(InsuranceError::Unauthorized),
            _ => {}
        }
        env.storage()
            .instance()
            .set(&symbol_short!("PAUSE_ADM"), &new_admin);
        Ok(())
    }

    pub fn pause(env: Env, caller: Address) -> Result<(), InsuranceError> {
        caller.require_auth();
        let admin = Self::get_pause_admin(&env).ok_or(InsuranceError::Unauthorized)?;
        if admin != caller {
            return Err(InsuranceError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&symbol_short!("PAUSED"), &true);
        Ok(())
    }

    pub fn unpause(env: Env, caller: Address) -> Result<(), InsuranceError> {
        caller.require_auth();
        let admin = Self::get_pause_admin(&env).ok_or(InsuranceError::Unauthorized)?;
        if admin != caller {
            return Err(InsuranceError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&symbol_short!("PAUSED"), &false);
        Ok(())
    }

    pub fn create_policy(
        env: Env,
        owner: Address,
        name: String,
        coverage_type: String,
        monthly_premium: i128,
        coverage_amount: i128,
    ) -> Result<u32, InsuranceError> {
        owner.require_auth();
        Self::require_not_paused(&env, pause_functions::CREATE_POLICY)?;

        if monthly_premium <= 0 || coverage_amount <= 0 {
            return Err(InsuranceError::InvalidAmount);
        }

        Self::extend_instance_ttl(&env);

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));

        let next_id = env
            .storage()
            .instance()
            .get(&symbol_short!("NEXT_ID"))
            .unwrap_or(0u32)
            + 1;

        let policy = InsurancePolicy {
            id: next_id,
            owner: owner.clone(),
            name,
            external_ref: None,
            coverage_type,
            monthly_premium,
            coverage_amount,
            active: true,
            next_payment_date: env.ledger().timestamp() + (30 * 86400),
            schedule_id: None,
            tags: Vec::new(&env),
        };

        policies.set(next_id, policy);
        env.storage()
            .instance()
            .set(&symbol_short!("POLICIES"), &policies);
        env.storage()
            .instance()
            .set(&symbol_short!("NEXT_ID"), &next_id);

        env.events().publish(
            (symbol_short!("insure"), InsuranceEvent::PolicyCreated),
            (next_id, owner),
        );

        Ok(next_id)
    }

    pub fn pay_premium(env: Env, caller: Address, policy_id: u32) -> Result<bool, InsuranceError> {
        caller.require_auth();
        Self::require_not_paused(&env, pause_functions::PAY_PREMIUM)?;
        Self::extend_instance_ttl(&env);

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));

        let mut policy = policies
            .get(policy_id)
            .ok_or(InsuranceError::PolicyNotFound)?;
        if policy.owner != caller {
            return Err(InsuranceError::Unauthorized);
        }
        if !policy.active {
            return Err(InsuranceError::PolicyInactive);
        }

        policy.next_payment_date = env.ledger().timestamp() + (30 * 86400);
        policies.set(policy_id, policy);
        env.storage()
            .instance()
            .set(&symbol_short!("POLICIES"), &policies);

        env.events().publish(
            (symbol_short!("insure"), InsuranceEvent::PremiumPaid),
            (policy_id, caller),
        );

        Ok(true)
    }

    pub fn batch_pay_premiums(
        env: Env,
        caller: Address,
        policy_ids: Vec<u32>,
    ) -> Result<u32, InsuranceError> {
        caller.require_auth();
        Self::require_not_paused(&env, pause_functions::PAY_PREMIUM)?;
        if policy_ids.len() > MAX_BATCH_SIZE {
            return Err(InsuranceError::BatchTooLarge);
        }
        Self::extend_instance_ttl(&env);

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));

        // Validate first to keep batch behavior atomic.
        for policy_id in policy_ids.iter() {
            let policy = policies
                .get(policy_id)
                .ok_or(InsuranceError::PolicyNotFound)?;
            if policy.owner != caller {
                return Err(InsuranceError::Unauthorized);
            }
            if !policy.active {
                return Err(InsuranceError::PolicyInactive);
            }
        }

        let next_due = env.ledger().timestamp() + (30 * 86400);
        let mut count = 0u32;
        for policy_id in policy_ids.iter() {
            let mut policy = policies
                .get(policy_id)
                .ok_or(InsuranceError::PolicyNotFound)?;
            policy.next_payment_date = next_due;
            policies.set(policy_id, policy);
            env.events().publish(
                (symbol_short!("insure"), InsuranceEvent::PremiumPaid),
                (policy_id, caller.clone()),
            );
            count += 1;
        }

        env.storage()
            .instance()
            .set(&symbol_short!("POLICIES"), &policies);
        Ok(count)
    }

    pub fn get_policy(env: Env, policy_id: u32) -> Option<InsurancePolicy> {
        let policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));
        policies.get(policy_id)
    }

    pub fn get_active_policies(env: Env, owner: Address, cursor: u32, limit: u32) -> PolicyPage {
        let limit = Self::clamp_limit(limit);
        let policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));

        let mut staging: Vec<(u32, InsurancePolicy)> = Vec::new(&env);
        for (id, p) in policies.iter() {
            if id <= cursor || p.owner != owner || !p.active {
                continue;
            }
            staging.push_back((id, p));
            if staging.len() > limit {
                break;
            }
        }

        Self::build_page(&env, staging, limit)
    }

    pub fn get_all_policies_for_owner(
        env: Env,
        owner: Address,
        cursor: u32,
        limit: u32,
    ) -> PolicyPage {
        let limit = Self::clamp_limit(limit);
        let policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));

        let mut staging: Vec<(u32, InsurancePolicy)> = Vec::new(&env);
        for (id, p) in policies.iter() {
            if id <= cursor || p.owner != owner {
                continue;
            }
            staging.push_back((id, p));
            if staging.len() > limit {
                break;
            }
        }

        Self::build_page(&env, staging, limit)
    }

    fn build_page(env: &Env, staging: Vec<(u32, InsurancePolicy)>, limit: u32) -> PolicyPage {
        let has_next = staging.len() > limit;
        let take = if has_next {
            staging.len() - 1
        } else {
            staging.len()
        };

        let mut items = Vec::new(env);
        for i in 0..take {
            if let Some((_, p)) = staging.get(i) {
                items.push_back(p);
            }
        }

        let mut next_cursor = 0u32;
        if has_next {
            if let Some((id, _)) = staging.get(take - 1) {
                next_cursor = id;
            }
        }

        PolicyPage {
            count: items.len(),
            items,
            next_cursor,
        }
    }

    pub fn get_total_monthly_premium(env: Env, owner: Address) -> i128 {
        let policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));

        let mut total = 0i128;
        for (_, p) in policies.iter() {
            if p.owner == owner && p.active {
                total += p.monthly_premium;
            }
        }
        total
    }

    pub fn deactivate_policy(
        env: Env,
        caller: Address,
        policy_id: u32,
    ) -> Result<bool, InsuranceError> {
        caller.require_auth();
        Self::require_not_paused(&env, pause_functions::DEACTIVATE)?;

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));

        let mut policy = policies
            .get(policy_id)
            .ok_or(InsuranceError::PolicyNotFound)?;
        if policy.owner != caller {
            return Err(InsuranceError::Unauthorized);
        }

        policy.active = false;
        policies.set(policy_id, policy);
        env.storage()
            .instance()
            .set(&symbol_short!("POLICIES"), &policies);

        env.events().publish(
            (symbol_short!("insure"), InsuranceEvent::PolicyDeactivated),
            (policy_id, caller),
        );

        Ok(true)
    }

    pub fn set_external_ref(
        env: Env,
        caller: Address,
        policy_id: u32,
        external_ref: Option<String>,
    ) -> Result<bool, InsuranceError> {
        caller.require_auth();

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));

        let mut policy = policies
            .get(policy_id)
            .ok_or(InsuranceError::PolicyNotFound)?;
        if policy.owner != caller {
            return Err(InsuranceError::Unauthorized);
        }

        policy.external_ref = external_ref;
        policies.set(policy_id, policy);
        env.storage()
            .instance()
            .set(&symbol_short!("POLICIES"), &policies);

        Ok(true)
    }

    pub fn create_premium_schedule(
        env: Env,
        owner: Address,
        policy_id: u32,
        next_due: u64,
        interval: u64,
    ) -> Result<u32, InsuranceError> {
        owner.require_auth();
        Self::require_not_paused(&env, pause_functions::CREATE_SCHED)?;

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));
        let mut policy = policies
            .get(policy_id)
            .ok_or(InsuranceError::PolicyNotFound)?;

        if policy.owner != owner {
            return Err(InsuranceError::Unauthorized);
        }
        if next_due <= env.ledger().timestamp() {
            return Err(InsuranceError::InvalidTimestamp);
        }

        let mut schedules: Map<u32, PremiumSchedule> = env
            .storage()
            .instance()
            .get(&symbol_short!("PREM_SCH"))
            .unwrap_or_else(|| Map::new(&env));

        let schedule_id = env
            .storage()
            .instance()
            .get(&symbol_short!("NEXT_PSCH"))
            .unwrap_or(0u32)
            + 1;

        let schedule = PremiumSchedule {
            id: schedule_id,
            owner: owner.clone(),
            policy_id,
            next_due,
            interval,
            recurring: interval > 0,
            active: true,
            created_at: env.ledger().timestamp(),
            last_executed: None,
            missed_count: 0,
        };

        schedules.set(schedule_id, schedule);
        policy.schedule_id = Some(schedule_id);
        policies.set(policy_id, policy);

        env.storage()
            .instance()
            .set(&symbol_short!("PREM_SCH"), &schedules);
        env.storage()
            .instance()
            .set(&symbol_short!("NEXT_PSCH"), &schedule_id);
        env.storage()
            .instance()
            .set(&symbol_short!("POLICIES"), &policies);

        Ok(schedule_id)
    }

    pub fn modify_premium_schedule(
        env: Env,
        caller: Address,
        schedule_id: u32,
        next_due: u64,
        interval: u64,
    ) -> Result<bool, InsuranceError> {
        caller.require_auth();
        Self::require_not_paused(&env, pause_functions::MODIFY_SCHED)?;

        if next_due <= env.ledger().timestamp() {
            return Err(InsuranceError::InvalidTimestamp);
        }

        let mut schedules: Map<u32, PremiumSchedule> = env
            .storage()
            .instance()
            .get(&symbol_short!("PREM_SCH"))
            .unwrap_or_else(|| Map::new(&env));

        let mut schedule = schedules
            .get(schedule_id)
            .ok_or(InsuranceError::ScheduleNotFound)?;
        if schedule.owner != caller {
            return Err(InsuranceError::Unauthorized);
        }

        schedule.next_due = next_due;
        schedule.interval = interval;
        schedule.recurring = interval > 0;
        schedules.set(schedule_id, schedule);
        env.storage()
            .instance()
            .set(&symbol_short!("PREM_SCH"), &schedules);

        Ok(true)
    }

    pub fn cancel_premium_schedule(
        env: Env,
        caller: Address,
        schedule_id: u32,
    ) -> Result<bool, InsuranceError> {
        caller.require_auth();
        Self::require_not_paused(&env, pause_functions::CANCEL_SCHED)?;

        let mut schedules: Map<u32, PremiumSchedule> = env
            .storage()
            .instance()
            .get(&symbol_short!("PREM_SCH"))
            .unwrap_or_else(|| Map::new(&env));

        let mut schedule = schedules
            .get(schedule_id)
            .ok_or(InsuranceError::ScheduleNotFound)?;
        if schedule.owner != caller {
            return Err(InsuranceError::Unauthorized);
        }

        schedule.active = false;
        schedules.set(schedule_id, schedule);
        env.storage()
            .instance()
            .set(&symbol_short!("PREM_SCH"), &schedules);

        Ok(true)
    }

    pub fn execute_due_premium_schedules(env: Env) -> Vec<u32> {
        let now = env.ledger().timestamp();
        let mut executed = Vec::new(&env);

        let mut schedules: Map<u32, PremiumSchedule> = env
            .storage()
            .instance()
            .get(&symbol_short!("PREM_SCH"))
            .unwrap_or_else(|| Map::new(&env));

        let mut policies: Map<u32, InsurancePolicy> = env
            .storage()
            .instance()
            .get(&symbol_short!("POLICIES"))
            .unwrap_or_else(|| Map::new(&env));

        for (schedule_id, mut schedule) in schedules.iter() {
            if !schedule.active || schedule.next_due > now {
                continue;
            }

            if let Some(mut policy) = policies.get(schedule.policy_id) {
                if policy.active {
                    policy.next_payment_date = now + (30 * 86400);
                    policies.set(schedule.policy_id, policy);
                }
            }

            schedule.last_executed = Some(now);
            if schedule.recurring && schedule.interval > 0 {
                let mut missed = 0u32;
                let mut next = schedule.next_due + schedule.interval;
                while next <= now {
                    missed += 1;
                    next += schedule.interval;
                }
                schedule.missed_count += missed;
                schedule.next_due = next;
            } else {
                schedule.active = false;
            }

            schedules.set(schedule_id, schedule);
            executed.push_back(schedule_id);
        }

        env.storage()
            .instance()
            .set(&symbol_short!("PREM_SCH"), &schedules);
        env.storage()
            .instance()
            .set(&symbol_short!("POLICIES"), &policies);
        executed
    }

    pub fn get_premium_schedules(env: Env, owner: Address) -> Vec<PremiumSchedule> {
        let schedules: Map<u32, PremiumSchedule> = env
            .storage()
            .instance()
            .get(&symbol_short!("PREM_SCH"))
            .unwrap_or_else(|| Map::new(&env));

        let mut result = Vec::new(&env);
        for (_, schedule) in schedules.iter() {
            if schedule.owner == owner {
                result.push_back(schedule);
            }
        }
        result
    }

    pub fn get_premium_schedule(env: Env, schedule_id: u32) -> Option<PremiumSchedule> {
        let schedules: Map<u32, PremiumSchedule> = env
            .storage()
            .instance()
            .get(&symbol_short!("PREM_SCH"))
            .unwrap_or_else(|| Map::new(&env));
        schedules.get(schedule_id)
    }
}

#[cfg(test)]
mod test;
