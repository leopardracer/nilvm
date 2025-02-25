use crate::selector::{Selector, SelectorParseError};
use serde::{
    de::{Error, SeqAccess, Unexpected, Visitor},
    Deserialize, Deserializer,
};
use std::fmt;

/// A NUC policy.
#[derive(Clone, Debug, PartialEq)]
pub enum Policy {
    Operator(OperatorPolicy),
    Connector(ConnectorPolicy),
}

impl Policy {
    /// Evaluate a policy on a JSON value.
    #[must_use]
    pub fn evaluate(&self, nuc_root: &serde_json::Value) -> bool {
        match self {
            Self::Operator(policy) => policy.evaluate(nuc_root),
            Self::Connector(policy) => policy.evaluate(nuc_root),
        }
    }
}

/// A policy that contains an operator.
#[derive(Clone, Debug, PartialEq)]
pub struct OperatorPolicy {
    selector: Selector,
    operator: Operator,
}

impl OperatorPolicy {
    fn evaluate(&self, value: &serde_json::Value) -> bool {
        let value = self.selector.apply(value);
        match &self.operator {
            Operator::Equals(expected) => value == expected,
            Operator::NotEquals(expected) => value != expected,
            Operator::AnyOf(options) => options.iter().any(|option| value == option),
        }
    }
}

impl From<OperatorPolicy> for Policy {
    fn from(operator: OperatorPolicy) -> Self {
        Self::Operator(operator)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Operator {
    Equals(serde_json::Value),
    NotEquals(serde_json::Value),
    AnyOf(Vec<serde_json::Value>),
}

/// A policy that contains a connector.
#[derive(Clone, Debug, PartialEq)]
pub enum ConnectorPolicy {
    And(Vec<Policy>),
    Or(Vec<Policy>),
    Not(Box<Policy>),
}

impl ConnectorPolicy {
    fn evaluate(&self, value: &serde_json::Value) -> bool {
        match self {
            ConnectorPolicy::And(conditions) => {
                !conditions.is_empty() && conditions.iter().all(|condition| condition.evaluate(value))
            }
            ConnectorPolicy::Or(conditions) => conditions.iter().any(|condition| condition.evaluate(value)),
            ConnectorPolicy::Not(policy) => !policy.evaluate(value),
        }
    }
}

impl From<ConnectorPolicy> for Policy {
    fn from(operator: ConnectorPolicy) -> Self {
        Self::Connector(operator)
    }
}

impl<'de> Deserialize<'de> for Policy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(PolicyVisitor)
    }
}

struct PolicyVisitor;

impl PolicyVisitor {
    fn build_selector<'de, A>(&self, seq: &mut A, encountered: usize) -> Result<Selector, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let selector = seq
            .next_element::<String>()?
            .ok_or_else(|| Error::invalid_length(encountered, &"an attribute selector"))?;
        selector.parse().map_err(|e: SelectorParseError| Error::invalid_value(Unexpected::Seq, &e.to_string().as_str()))
    }

    fn build_simple_operator<'de, A>(self, mut seq: A, op: String) -> Result<OperatorPolicy, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let selector = self.build_selector(&mut seq, 1)?;
        let value = seq.next_element::<serde_json::Value>()?.ok_or_else(|| Error::invalid_length(2, &"a value"))?;
        let operator = match op.as_str() {
            "==" => Operator::Equals(value),
            "!=" => Operator::NotEquals(value),
            _ => return Err(Error::custom("unhandled operator")),
        };
        Ok(OperatorPolicy { selector, operator })
    }

    fn build_list_operator<'de, A>(self, mut seq: A, op: String) -> Result<OperatorPolicy, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let selector = self.build_selector(&mut seq, 1)?;
        let values = seq
            .next_element::<Vec<serde_json::Value>>()?
            .ok_or_else(|| Error::invalid_length(2, &"a list of values"))?;
        let operator = match op.as_str() {
            "anyOf" => Operator::AnyOf(values),
            _ => return Err(Error::custom("unhandled operator")),
        };
        Ok(OperatorPolicy { selector, operator })
    }

    fn build_simple_connector<'de, A>(self, mut seq: A, connector: String) -> Result<ConnectorPolicy, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let policy = seq.next_element::<Box<Policy>>()?.ok_or_else(|| Error::invalid_length(1, &"a policy"))?;
        let connector = match connector.as_str() {
            "not" => ConnectorPolicy::Not(policy),
            _ => return Err(Error::custom("unhandled operator")),
        };
        Ok(connector)
    }

    fn build_list_connector<'de, A>(self, mut seq: A, connector: String) -> Result<ConnectorPolicy, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let policies =
            seq.next_element::<Vec<Policy>>()?.ok_or_else(|| Error::invalid_length(1, &"a list of policies"))?;
        let connector = match connector.as_str() {
            "and" => ConnectorPolicy::And(policies),
            "or" => ConnectorPolicy::Or(policies),
            _ => return Err(Error::custom("unhandled operator")),
        };
        Ok(connector)
    }
}

impl<'de> Visitor<'de> for PolicyVisitor {
    type Value = Policy;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a valid policy array")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let s = seq.next_element::<String>()?.ok_or_else(|| Error::invalid_length(0, &self))?;
        match s.as_str() {
            "==" | "!=" => self.build_simple_operator(seq, s).map(Into::into),
            "anyOf" => self.build_list_operator(seq, s).map(Policy::Operator),
            "and" | "or" => self.build_list_connector(seq, s).map(Into::into),
            "not" => self.build_simple_connector(seq, s).map(Into::into),
            _ => Err(Error::custom(format!("invalid policy operator '{s}'"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde_json::{json, Value};

    mod op {
        use super::*;

        pub(super) fn eq(selector: &str, value: Value) -> Policy {
            OperatorPolicy { selector: selector.parse().expect("invalid selector"), operator: Operator::Equals(value) }
                .into()
        }

        pub(super) fn ne(selector: &str, value: Value) -> Policy {
            OperatorPolicy {
                selector: selector.parse().expect("invalid selector"),
                operator: Operator::NotEquals(value),
            }
            .into()
        }

        pub(super) fn any_of(selector: &str, values: &[Value]) -> Policy {
            OperatorPolicy {
                selector: selector.parse().expect("invalid selector"),
                operator: Operator::AnyOf(values.to_vec()),
            }
            .into()
        }

        pub(super) fn and(policies: &[Policy]) -> Policy {
            ConnectorPolicy::And(policies.to_vec()).into()
        }

        pub(super) fn or(policies: &[Policy]) -> Policy {
            ConnectorPolicy::Or(policies.to_vec()).into()
        }

        pub(super) fn not(policy: Policy) -> Policy {
            ConnectorPolicy::Not(policy.into()).into()
        }
    }

    #[rstest]
    #[case::eq(json!(["==", ".foo", {"bar": 42}]), op::eq(".foo", json!({"bar": 42})))]
    #[case::ne(json!(["!=", ".foo", {"bar": 42}]), op::ne(".foo", json!({"bar": 42})))]
    #[case::anyof1(json!(["anyOf", ".foo", [42, "hi"]]), op::any_of(".foo", &[json!(42), json!("hi")]))]
    #[case::anyof2(json!(["anyOf", ".foo", [{"foo": 42}]]), op::any_of(".foo", &[json!({"foo": 42})]))]
    #[case::and(
        json!(["and", [["==", ".foo", 42], ["!=", ".bar", false]]]),
        op::and(&[op::eq(".foo", json!(42)), op::ne(".bar", json!(false))]))
    ]
    #[case::or(
        json!(["or", [["==", ".foo", 42], ["!=", ".bar", false]]]),
        op::or(&[op::eq(".foo", json!(42)), op::ne(".bar", json!(false))]))
    ]
    #[case::not(
        json!(["not", ["==", ".foo", 42]]),
        op::not(op::eq(".foo", json!(42))))
    ]
    #[case::nested(
        json!(["or", [["==", ".foo", 42], ["and", [["!=", ".bar", 1337], ["not", ["==", ".tar", true]]]]]]),
        op::or(&[op::eq(".foo", json!(42)), op::and(&[op::ne(".bar", json!(1337)), op::not(op::eq(".tar", json!(true)))])]))
    ]
    fn valid_policies(#[case] input: Value, #[case] expected: Policy) {
        let policy: Policy = serde_json::from_value(input).expect("parse failed");
        assert_eq!(policy, expected);
    }

    #[rstest]
    #[case::empty(json!([]))]
    #[case::only_op(json!(["=="]))]
    #[case::invalid_op(json!(["hi", ".foo", []]))]
    #[case::no_value1(json!(["==", ".foo"]))]
    #[case::no_value2(json!(["!=", ".foo"]))]
    #[case::no_value3(json!(["anyOf", ".foo"]))]
    #[case::anyof_no_vec(json!(["anyOf", ".foo", json!(42)]))]
    #[case::and_no_vec(json!(["and"]))]
    #[case::or_no_vec(json!(["or"]))]
    #[case::not_no_policy(json!(["not"]))]
    #[case::and_bogus_vec(json!(["and", ["hi"]]))]
    #[case::and_bogus_policy(json!(["and", [["hi"]]]))]
    #[case::not_bogus_policy(json!(["not", "hi"]))]
    fn invalid_policies(#[case] input: Value) {
        serde_json::from_value::<Policy>(input).expect_err("parse succeeded");
    }

    // A set of conditions that evaluate to true for the JSON in this test.
    #[rstest]
    #[case::eq(op::eq(".name.first", json!("bob")))]
    #[case::ne(op::ne(".name.first", json!("john")))]
    #[case::eq(op::eq(".name", json!({"first": "bob", "last": "smith"})))]
    #[case::eq(op::eq(".", json!({"name": {"first": "bob", "last": "smith"}, "age": 42})))]
    #[case::not_eq(op::not(op::eq(".age", json!(150))))]
    #[case::any_of(op::any_of(".name.first", &[json!("john"), json!("bob")]))]
    #[case::and(op::and(&[op::eq(".age", json!(42)), op::eq(".name.first", json!("bob"))]))]
    #[case::or_short_circuit(op::or(&[op::eq(".age", json!(42)), op::eq(".age", json!(100))]))]
    #[case::or_long_circuit(op::or(&[op::eq(".age", json!(100)), op::eq(".age", json!(42))]))]
    fn evaluation_matches(#[case] policy: Policy) {
        let value = json!({
            "name": {
                "first": "bob",
                "last": "smith",
            },
            "age": 42,
        });
        assert!(policy.evaluate(&value));
    }

    // A set of conditions that evaluate to false for the JSON in this test.
    #[rstest]
    #[case::eq(op::eq(".name.first", json!("john")))]
    #[case::ne(op::ne(".name.first", json!("bob")))]
    #[case::eq(op::eq(".name", json!({"first": "john", "last": "smith"})))]
    #[case::eq(op::eq(".", json!({"name": {"first": "bob", "last": "smith"}, "age": 100})))]
    #[case::not_eq(op::not(op::eq(".age", json!(42))))]
    #[case::any_of(op::any_of(".name.first", &[json!("john"), json!("jack")]))]
    #[case::and1(op::and(&[op::eq(".age", json!(150)), op::eq(".name.first", json!("bob"))]))]
    #[case::and2(op::and(&[op::eq(".age", json!(42)), op::eq(".name.first", json!("john"))]))]
    #[case::empty_and(op::and(&[]))]
    #[case::or_short_circuit(op::or(&[op::eq(".age", json!(101)), op::eq(".age", json!(100))]))]
    #[case::or_long_circuit(op::or(&[op::eq(".age", json!(100)), op::eq(".age", json!(101))]))]
    #[case::empty_or(op::or(&[]))]
    fn evaluation_does_not_match(#[case] policy: Policy) {
        let value = json!({
            "name": {
                "first": "bob",
                "last": "smith",
            },
            "age": 42,
        });
        assert!(!policy.evaluate(&value));
    }
}
