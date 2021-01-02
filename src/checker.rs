//! Private module for selective re-export.

mod bfs;
use crate::{fingerprint, Fingerprint, Expectation, Model};
mod dfs;
mod explorer;
mod visitor;
use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::time::Instant;

pub use visitor::*;

/// A [`Model`] [`Checker`] builder. Instantiable via the [`Model::checker`] method.
///
/// # Example
///
/// ```
/// # use stateright::*; let model = ();
/// model.checker().threads(4).spawn_dfs().join().assert_properties();
/// ```
#[must_use = "This code constructs a builder, not a checker. \
              Consider calling spawn_bfs() or spawn_dfs()."]
pub struct CheckerBuilder<M: Model> {
    model: M,
    target_generated_count: Option<NonZeroUsize>,
    thread_count: usize,
    visitor: Option<Box<dyn CheckerVisitor<M> + Send + Sync>>,
}
impl<M: Model> CheckerBuilder<M> {
    pub(crate) fn new(model: M) -> Self {
        Self {
            model,
            target_generated_count: None,
            thread_count: 1,
            visitor: None,
        }
    }

    /// Starts a web service for interactively exploring a model ([demo](http://demo.stateright.rs:3000/)).
    ///
    /// ![Stateright Explorer screenshot](https://raw.githubusercontent.com/stateright/stateright/master/explorer.png)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use stateright::{Checker, Model};
    ///
    /// #[derive(Clone, Debug, Hash)]
    /// enum FizzBuzzAction { Fizz, Buzz, FizzBuzz }
    /// #[derive(Clone)]
    /// struct FizzBuzzModel { max: usize }
    ///
    /// impl Model for FizzBuzzModel {
    ///     type State = Vec<(usize, Option<FizzBuzzAction>)>;
    ///     type Action = Option<FizzBuzzAction>;
    ///     fn init_states(&self) -> Vec<Self::State> {
    ///         vec![Vec::new()]
    ///     }
    ///     fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
    ///         actions.push(
    ///             if state.len() % 15 == 0 {
    ///                 Some(FizzBuzzAction::FizzBuzz)
    ///             } else if state.len() % 5 == 0 {
    ///                 Some(FizzBuzzAction::Buzz)
    ///             } else if state.len() % 3 == 0 {
    ///                 Some(FizzBuzzAction::Fizz)
    ///             } else {
    ///                 None
    ///             });
    ///     }
    ///     fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
    ///         let mut state = state.clone();
    ///         state.push((state.len(), action));
    ///         Some(state)
    ///     }
    ///     fn within_boundary(&self, state: &Self::State) -> bool {
    ///         state.len() <= self.max
    ///     }
    /// }
    ///
    /// let _ = FizzBuzzModel { max: 30 }.checker().serve("localhost:3000");
    /// ```
    ///
    /// # API
    ///
    /// - `GET /` returns a web browser UI as HTML.
    /// - `GET /.status` returns information about the model checker status.
    /// - `GET /.states` returns available initial states and fingerprints.
    /// - `GET /.states/{fingerprint1}/{fingerprint2}/...` follows the specified
    ///    path of fingerprints and returns available actions with resulting
    ///    states and fingerprints.
    /// - `GET /.states/.../{invalid-fingerprint}` returns 404.
    pub fn serve(self, addresses: impl std::net::ToSocketAddrs) -> std::sync::Arc<impl Checker<M>>
    where M: 'static + Model + Send + Sync,
          M::Action: Debug + Send + Sync,
          M::State: Debug + Hash + Send + Sync,
    {
        explorer::serve(self, addresses)
    }

    /// Spawns a breadth-first search model checker. This traversal strategy uses more memory than
    /// [`CheckerBuilder::spawn_dfs`] but will find the shortest [`Path`] to each discovery if
    /// checking is single threadeded (the default behavior, which [`CheckerBuilder::threads`]
    /// overrides).
    ///
    /// This call does not block the current thread. Call [`Checker::join`] to block until checking
    /// completes.
    #[must_use = "Checkers run on background threads. \
                  Consider calling join() or report(...), for example."]
    pub fn spawn_bfs(self) -> impl Checker<M>
    where M: Model + Send + Sync + 'static,
          M::State: Hash + Send + Sync + 'static,
    {
        bfs::BfsChecker::spawn(self)
    }

    /// Spawns a depth-first search model checker. This traversal strategy uses dramatically less
    /// memory than [`CheckerBuilder::spawn_bfs`] at the cost of not finding the shortest [`Path`]
    /// to each discovery.
    ///
    /// This call does not block the current thread. Call [`Checker::join`] to block until
    /// checking completes.
    #[must_use = "Checkers run on background threads. \
                  Consider calling join() or report(...), for example."]
    pub fn spawn_dfs(self) -> impl Checker<M>
    where M: Model + Send + Sync + 'static,
          M::State: Hash + Send + Sync + 'static,
    {
        dfs::DfsChecker::spawn(self)
    }

    /// Sets the number of states that the checker should aim to generate. For performance reasons
    /// the checker may exceed this number, but it will never generate fewer states if more exist.
    pub fn target_generated_count(self, target_generated_count: usize) -> Self {
        Self { target_generated_count: NonZeroUsize::new(target_generated_count), .. self }
    }

    /// Sets the number of threads available for model checking. For maximum performance this
    /// should match the number of cores.
    pub fn threads(self, thread_count: usize) -> Self {
        Self { thread_count, .. self }
    }

    /// Indicates a function to be run on each evaluated state.
    pub fn visitor(self, visitor: impl CheckerVisitor<M> + Send + Sync + 'static) -> Self {
        Self { visitor: Some(Box::new(visitor)), .. self }
    }
}

/// A path of states including actions. i.e. `state --action--> state ... --action--> state`.
///
/// You can convert to a `Vec<_>` with [`path.into_vec()`]. If you only need the actions, then use
/// [`path.into_actions()`].
///
/// [`path.into_vec()`]: Path::into_vec
/// [`path.into_actions()`]: Path::into_actions
#[derive(Clone, Debug, PartialEq)]
pub struct Path<State, Action>(Vec<(State, Option<Action>)>);
impl<State, Action> Path<State, Action> {
    /// Constructs a path from a model and a sequence of fingerprints.
    fn from_fingerprints<M>(model: &M, mut fingerprints: VecDeque<Fingerprint>) -> Self
    where M: Model<State = State, Action = Action>,
          M::State: Hash,
    {
        let init_print = match fingerprints.pop_front() {
            Some(init_print) => init_print,
            None => panic!("empty path is invalid"),
        };
        let mut last_state = model.init_states().into_iter()
            .find(|s| {
                fingerprint(&s) == init_print
            })
            .expect("no init state matches fingerprint");
        let mut output = Vec::new();
        while let Some(next_fp) = fingerprints.pop_front() {
            let mut actions = Vec::new();
            model.actions(
                &last_state,
                &mut actions);

            let (action, next_state) = model
                .next_steps(&last_state).into_iter()
                .find_map(|(a,s)| {
                    if fingerprint(&s) == next_fp {
                        Some((a, s))
                    } else {
                        None
                    }
                }).expect("no next state matches fingerprint");
            output.push((last_state, Some(action)));

            last_state = next_state;
        }
        output.push((last_state, None));
        Path(output)
    }

    /// Constructs a path from a model, initial state, and a sequence of actions. Panics for inputs
    /// unreachable via the model.
    pub fn from_actions<'a, M>(
        model: &M, init_state: State, actions: impl IntoIterator<Item = &'a Action>) -> Option<Self>
    where M: Model<State = State, Action = Action>,
          State: PartialEq,
          Action: PartialEq + 'a,
    {
        let mut output = Vec::new();
        if !model.init_states().contains(&init_state) { return None }
        let mut prev_state = init_state;
        for action in actions {
            let (action, next_state) = match model.next_steps(&prev_state)
                .into_iter().find(|(a, _)| a == action)
            {
                None => return None,
                Some(found) => found,
            };
            output.push((prev_state, Some(action)));
            prev_state = next_state;
        }
        output.push((prev_state, None));

        Some(Path(output))
    }

    /// Determines the final state associated with a particular fingerprint path.
    fn final_state<M>(model: &M, mut fingerprints: VecDeque<Fingerprint>) -> Option<M::State>
    where M: Model<State = State, Action = Action>,
          M::State: Hash,
    {
        let init_print = match fingerprints.pop_front() {
            Some(init_print) => init_print,
            None => return None,
        };
        let mut matching_state =
            match model.init_states().into_iter().find(|s| fingerprint(&s) == init_print) {
                Some(matching_state) => matching_state,
                None => return None,
            };
        while let Some(next_print) = fingerprints.pop_front() {
            matching_state =
                match model.next_states(&matching_state).into_iter().find(|s| fingerprint(&s) == next_print) {
                    Some(matching_state) => matching_state,
                    None => return None,
                };
        }
        Some(matching_state)
    }

    /// Extracts the last state.
    pub fn last_state(&self) -> &State {
        &self.0.last().unwrap().0
    }

    /// Extracts the states.
    pub fn into_states(self) -> Vec<State> {
        self.0.into_iter().map(|(s, _a)| s).collect()
    }

    /// Extracts the actions.
    pub fn into_actions(self) -> Vec<Action> {
        self.0.into_iter().filter_map(|(_s, a)| a).collect()
    }

    /// Convenience method for `Into<Vec<_>>`.
    pub fn into_vec(self) -> Vec<(State, Option<Action>)> {
        self.into()
    }

    /// Encodes the path as a sequence of opaque "fingerprints" delimited by forward
    /// slash (`/`) characters.
    pub fn encode(&self) -> String where State: Hash {
        self.0.iter()
            .map(|(s, _a)| format!("{}", fingerprint(s)))
            .collect::<Vec<String>>()
            .join("/")
    }
}
impl<State, Action> Into<Vec<(State, Option<Action>)>> for Path<State, Action> {
    fn into(self) -> Vec<(State, Option<Action>)> { self.0 }
}
impl<State, Action> Display for Path<State, Action> 
where Action: Debug,
      State: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        writeln!(f, "Path[{}]:", self.0.len() - 1)?;
        for (_state, action) in &self.0 {
            if let Some(action) = action {
                writeln!(f, "- {:?}", action)?;
            }
        }
        Ok(())
    }
}

/// Implementations perform [`Model`] checking.
///
/// Call [`Model::checker`] to instantiate a [`CheckerBuilder`]. Then call
/// [`CheckerBuilder::spawn_dfs`] or [`CheckerBuilder::spawn_bfs`].
pub trait Checker<M: Model> {
    /// Returns a reference to this checker's [`Model`].
    fn model(&self) -> &M;

    /// Indicates how many states have been generated.
    fn generated_count(&self) -> usize;

    /// Returns a map from property name to corresponding "discovery" (indicated
    /// by a [`Path`]).
    fn discoveries(&self) -> HashMap<&'static str, Path<M::State, M::Action>>;

    /// Blocks the current thread until checking [`is_done`] or each thread evaluates
    /// a specified maximum number of states.
    ///
    /// [`is_done`]: Self::is_done
    fn join(self) -> Self;

    /// Indicates that either all properties have associated discoveries or all reachable states
    /// have been visited.
    fn is_done(&self) -> bool;

    /// Looks up a discovery by property name. Panics if the property does not exist.
    fn discovery(&self, name: &'static str) -> Option<Path<M::State, M::Action>> {
        self.discoveries().remove(name)
    }

    /// Periodically emits a status message.
    fn report(self, w: &mut impl std::io::Write) -> Self
    where M::Action: Debug,
          M::State: Debug,
          Self: Sized,
    {
        // Start with the checking status.
        let method_start = Instant::now();
        while !self.is_done() {
            let _ = writeln!(w, "Checking. generated={}", self.generated_count());
            std::thread::sleep(std::time::Duration::from_millis(1_000));
        }
        let _ = writeln!(w, "Done. generated={}, sec={}",
                 self.generated_count(),
                 method_start.elapsed().as_secs());

        // Finish with a discovery summary.
        for (name, path) in self.discoveries() {
            let _ = write!(w, "Discovered \"{}\" {} {}",
                           name, self.discovery_classification(name), path);
        }

        self
    }

    /// Indicates whether a discovery is an `"example"` or `"counterexample"`.
    fn discovery_classification(&self, name: &str) -> &'static str {
        let properties = self.model().properties();
        let property = properties.iter().find(|p| p.name == name).unwrap();
        match property.expectation {
            Expectation::Always | Expectation::Eventually => "counterexample",
            Expectation::Sometimes => "example",
        }
    }

    /// A helper that verifies examples exist for all `sometimes` properties and no counterexamples
    /// exist for any `always`/`eventually` properties.
    fn assert_properties(&self)
    where M::Action: Debug,
          M::State: Debug,
    {
        for p in self.model().properties() {
            match p.expectation {
                Expectation::Always => self.assert_no_discovery(p.name),
                Expectation::Eventually => self.assert_no_discovery(p.name),
                Expectation::Sometimes => { self.assert_any_discovery(p.name); },
            }
        }
    }

    /// Panics if a particular discovery is not found.
    fn assert_any_discovery(&self, name: &'static str) -> Path<M::State, M::Action> {
        if let Some(found) = self.discovery(name) { return found }
        assert!(self.is_done(),
                "Discovery for '{}' not found, but model checking is incomplete.", name);
        panic!("Discovery for '{}' not found.", name);
    }

    /// Panics if a particular discovery is found.
    fn assert_no_discovery(&self, name: &'static str)
    where M::Action: Debug,
          M::State: Debug,
    {
        if let Some(found) = self.discovery(name) {
            panic!("Unexpected \"{}\" {} {}Last state: {:?}\n",
                   name, self.discovery_classification(name), found, found.last_state());
        }
        assert!(self.is_done(),
                "Discovery for '{}' not found, but model checking is incomplete.",
                name);
    }

    /// Panics if the specified actions do not result in a discovery for the specified property
    /// name.
    fn assert_discovery(&self, name: &'static str, actions: Vec<M::Action>)
    where M::State: Debug + PartialEq,
          M::Action: Debug + PartialEq,
    {
        let mut additional_info: Vec<&'static str> = Vec::new();

        let found = self.assert_any_discovery(name);
        for init_state in self.model().init_states() {
            if let Some(path) = Path::from_actions(self.model(), init_state, &actions) {
                let property = self.model().property(name);
                match property.expectation {
                    Expectation::Always => {
                        if !(property.condition)(self.model(), path.last_state()) { return }
                    }
                    Expectation::Eventually => {
                        let states = path.into_states();
                        let is_liveness_satisfied = states.iter().any(|s| {
                            (property.condition)(self.model(), s)
                        });
                        let is_path_terminal = {
                            let mut actions = Vec::new();
                            self.model().actions(&states.last().unwrap(), &mut actions);
                            actions.is_empty()
                        };
                        if !is_liveness_satisfied && is_path_terminal { return }
                        if is_liveness_satisfied {
                            additional_info.push("incorrect counterexample satisfies eventually property");
                        }
                        if !is_path_terminal {
                            additional_info.push("incorrect counterexample is nonterminal");
                        }
                    }
                    Expectation::Sometimes => {
                        if (property.condition)(self.model(), path.last_state()) { return }
                    }
                }
            }
        }
        let additional_info = if additional_info.is_empty() {
            "".to_string()
        } else {
            format!(" ({})", additional_info.join("; "))
        };
        panic!("Invalid discovery for '{}'{}, but a valid one was found. found={:?}",
               name, additional_info, found.into_actions());
    }
}

// EventuallyBits tracks one bit per 'eventually' property being checked. Properties are assigned
// bit-numbers just by counting the 'eventually' properties up from 0 in the properties list. If a
// bit is present in a bitset, the property has _not_ been found on this path yet. Bits are removed
// from the propagating bitset when we find a state satisfying an `eventually` property; these
// states are not considered discoveries. Only if we hit the "end" of a path (i.e. return to a known
// state / no further state) with any of these bits still 1, the path is considered a discovery,
// a counterexample to the property.
type EventuallyBits = id_set::IdSet;

#[cfg(test)]
mod test_eventually_property_checker {
    use crate::{Checker, Property};
    use crate::test_util::dgraph::DGraph;

    fn eventually_odd() -> Property<DGraph> {
        Property::eventually("odd", |_, s| s % 2 == 1)
    }

    #[test]
    fn can_validate() {
        DGraph::with_property(eventually_odd())
            .with_path(vec![1])        // satisfied at terminal init
            .with_path(vec![2, 3])     // satisfied at nonterminal init
            .with_path(vec![2, 6, 7])  // satisfied at terminal next
            .with_path(vec![4, 9, 10]) // satisfied at nonterminal next
            .check().assert_properties();
        // Repeat with distinct state spaces since stateful checking skips visited states (which we
        // don't expect here, but this is defense in depth).
        DGraph::with_property(eventually_odd())
            .with_path(vec![1]).check().assert_properties();
        DGraph::with_property(eventually_odd())
            .with_path(vec![2, 3]).check().assert_properties();
        DGraph::with_property(eventually_odd())
            .with_path(vec![2, 6, 7]).check().assert_properties();
        DGraph::with_property(eventually_odd())
            .with_path(vec![4, 9, 10]).check().assert_properties();
    }

    #[test]
    fn can_discover_counterexample() { // i.e. can falsify
        assert_eq!(
            DGraph::with_property(eventually_odd())
                .with_path(vec![0, 1])
                .with_path(vec![0, 2])
                .check().discovery("odd").unwrap().into_states(),
            vec![0, 2]);
        assert_eq!(
            DGraph::with_property(eventually_odd())
                .with_path(vec![0, 1])
                .with_path(vec![2, 4])
                .check().discovery("odd").unwrap().into_states(),
            vec![2, 4]);
        assert_eq!(
            DGraph::with_property(eventually_odd())
                .with_path(vec![0, 1, 4, 6])
                .with_path(vec![2, 4, 8])
                .check().discovery("odd").unwrap().into_states(),
            vec![2, 4, 6]);
    }

    #[test]
    fn fixme_can_miss_counterexample_when_revisiting_a_state() { // i.e. incorrectly verify
        assert_eq!(
            DGraph::with_property(eventually_odd())
                .with_path(vec![0, 2, 4, 2]) // cycle
                .check().discovery("odd"),
            None); // FIXME: `unwrap().into_states()` should be [0, 2, 4, 2]
        assert_eq!(
            DGraph::with_property(eventually_odd())
                .with_path(vec![0, 2, 4])
                .with_path(vec![1, 4, 6]) // revisiting 4
                .check().discovery("odd"),
            None); // FIXME: `unwrap().into_states()` should be [0, 2, 4, 6]
    }
}

#[cfg(test)]
mod test_path {
    use super::*;
    use crate::test_util::linear_equation_solver::LinearEquation;

    #[test]
    fn can_build_path_from_fingerprints() {
        let fp = |a: u8, b: u8| fingerprint(&(a, b));
        let model = LinearEquation { a: 2, b: 10, c: 14 };
        let fingerprints = VecDeque::from(vec![
            fp(0, 0),
            fp(0, 1),
            fp(1, 1),
            fp(2, 1), // final state
        ]);
        let path = Path::from_fingerprints(&model, fingerprints.clone());
        assert_eq!(
            path.last_state(),
            &(2,1));
        assert_eq!(
            path.last_state(),
            &Path::final_state(&model, fingerprints).unwrap());
    }
}

#[cfg(test)]
mod test_report {
    use super::*;
    use crate::test_util::linear_equation_solver::LinearEquation;

    #[test]
    fn report_includes_property_names_and_paths() {
        // The assertions use `starts_with` to omit timing since it varies.

        // BFS
        let mut written: Vec<u8> = Vec::new();
        LinearEquation { a: 2, b: 10, c: 14 }.checker()
            .spawn_bfs().report(&mut written);
        let output = String::from_utf8(written).unwrap();
        assert!(
            output.starts_with("\
                Checking. generated=1\n\
                Done. generated=12, sec="),
            "Output did not start as expected (see test). output={:?}`", output);
        assert!(
            output.ends_with("\
                Discovered \"solvable\" example Path[3]:\n\
                - IncreaseX\n\
                - IncreaseX\n\
                - IncreaseY\n"),
            "Output did not end as expected (see test). output={:?}`", output);

        // DFS
        let mut written: Vec<u8> = Vec::new();
        LinearEquation { a: 2, b: 10, c: 14 }.checker()
            .spawn_dfs().report(&mut written);
        let output = String::from_utf8(written).unwrap();
        assert!(
            output.starts_with("\
                Checking. generated=1\n\
                Done. generated=55, sec="),
            "Output did not start as expected (see test). output={:?}`", output);
        assert!(
            output.ends_with("\
                Discovered \"solvable\" example Path[27]:\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n\
                - IncreaseY\n"),
            "Output did not end as expected (see test). output={:?}`", output);
    }
}
