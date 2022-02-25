//! A filtering library for extracting parcels from a bindle.
//!
//! A bindle's invoice may contain many different parcels. And those parcels may have
//! groups and features associated with them. This library provides a way to filter a list
//! of parcels, returning only the specific parcels applicable to the given scenario.
//!
//! The recommended way of running a filter is using the `BindleFilter` builder:
//!
//! ```
//! use bindle::filters::BindleFilter;
//!
//! let toml = r#"
//!    bindleVersion = "1.0.0"
//!
//!    [bindle]
//!    name = "test/is-disabled"
//!    version = "0.1.0"
//!
//!    [[group]]
//!    name = "is_required"
//!    required = true
//!
//!    [[group]]
//!    name = "is_optional"
//!
//!    [[group]]
//!    name = "also_optional"
//!
//!    [[parcel]]
//!    [parcel.label]
//!    name = "first"
//!    sha256 = "12345"
//!    mediaType = "application/octet-stream"
//!    size = 123
//!    [parcel.conditions]
//!    memberOf = ["is_required"]
//!
//!    [[parcel]]
//!    [parcel.label]
//!    name = "second"
//!    sha256 = "4321"
//!    mediaType = "application/octet-stream"
//!    size = 321
//!    [parcel.conditions]
//!    memberOf = ["is_optional", "also_optional"]
//!
//!    [[parcel]]
//!    [parcel.label]
//!    name = "third"
//!    sha256 = "4321"
//!    mediaType = "application/octet-stream"
//!    size = 321
//!    [parcel.conditions]
//!    memberOf = ["also_optional"]
//!    "#;
//! let inv: bindle::Invoice = toml::from_str(toml).expect("test invoice parsed");
//!
//! let filter = BindleFilter::new(&inv).filter();
//! assert_eq!(1, filter.len());
//! ```
//!
//! It can also be used for enabling or disabling parcel features. For example, here's
//! an invoice that uses features:
//!
//! ```
//! use bindle::filters::BindleFilter;
//! let toml = r#"
//! bindleVersion = "1.0.0"
//! [bindle]
//! name = "test/activate"
//! version = "0.1.0"
//!
//! [[parcel]]
//! [parcel.label]
//! name = "narwhal_handler"
//! sha256 = "12345"
//! mediaType = "application/octet-stream"
//! size = 123
//! [parcel.label.feature.testing]
//! animal = "narwhal"
//! color = "blue"
//!
//! [[parcel]]
//! [parcel.label]
//! name = "unicorn_handler"
//! sha256 = "4321"
//! mediaType = "application/octet-stream"
//! size = 321
//! [parcel.label.feature.testing]
//! animal = "unicorn"
//! color = "blue"
//!
//! [[parcel]]
//! [parcel.label]
//! name = "default_thinger"
//! sha256 = "5432"
//! mediaType = "application/octet-stream"
//! size = 321
//! "#;
//! let inv: bindle::Invoice = toml::from_str(toml).expect("test invoice parsed");
//! let filter = BindleFilter::new(&inv)
//!     .activate_feature("testing", "animal", "narwhal")
//!     .filter();
//! assert_eq!(2, filter.len());
//! ```
use std::collections::HashSet;

use crate::{Invoice, Parcel};

/// A convenience representation of a feature as a member of a group with a name/value
/// pair attached.
#[derive(Clone)]
struct FeatureReference {
    group: String,
    name: String,
    value: String,
}

/// BindleFilter walks an invoice and resolves a list of parcels.
///
/// A bindle may define many parcels, some of which are to be included by default, and
/// others which are members of groups that are only conditionally included. Parcels also
/// have features attached to them. A feature can be turned on or off, and this will impact
/// which list of parcels are considered the correct ones to pass on to the runtime.
///
/// The filter can be used to retrieve the list of parcels that satisfies a set of
/// requirements. For example, use this to activate or deactivate features. You can also
/// include or exclude groups.
pub struct BindleFilter<'a> {
    // The invoice that we operate on.
    invoice: &'a Invoice,
    groups: HashSet<String>,
    exclude_groups: HashSet<String>,
    features: Vec<FeatureReference>,
    exclude_features: Vec<FeatureReference>,
}

impl<'a> BindleFilter<'a> {
    pub fn new(invoice: &'a Invoice) -> Self {
        Self {
            invoice,
            groups: HashSet::new(),
            exclude_groups: HashSet::new(),
            features: vec![],
            exclude_features: vec![],
        }
    }
    /// Explicitly enable the given group.
    ///
    /// Note that some groups may be enabled in virtue of a requirement condition
    /// (e.g. a parcel requires a group). This has no impact for that condition.
    pub fn with_group(&mut self, group_name: &str) -> &mut Self {
        self.groups.insert(group_name.to_owned());
        self
    }
    /// Explicitly disable the given group.
    ///
    /// Note that some groups may be enabled in virtue of a requirement condition
    /// (e.g. a parcel requires a group). This function will not override a `require`
    /// condition on a parcel, as doing so will break the entire requirement chain
    /// for the list. However, it will remove a group whose `required` field is set to
    /// true.
    pub fn without_group(&mut self, group_name: &str) -> &mut Self {
        self.exclude_groups.insert(group_name.to_owned());
        self
    }
    /// Activate a feature by group, name, and value.
    ///
    /// This corresponds to the TOML:
    ///
    /// [parcel.label.feature.GROUP]
    /// NAME = "VALUE"
    ///
    /// This will mark parcels with this feature as "activated", meaning they will be
    /// returned in the filter result if something else does not remove them.
    pub fn activate_feature(&mut self, group: &str, name: &str, value: &str) -> &mut Self {
        // This is a hack to remove duplicate group/name pairs.
        self.features = self
            .features
            .iter()
            .filter(|i| !(i.name == name && i.group == group))
            .cloned()
            .collect();

        self.features.push(FeatureReference {
            group: group.to_owned(),
            name: name.to_owned(),
            value: value.to_owned(),
        });
        self
    }

    /// Deactivate a feature by group, name, and value.
    ///
    /// This corresponds to the TOML:
    ///
    /// [parcel.label.feature.GROUP]
    /// NAME = "VALUE"
    ///
    /// If a feature is activated and deactivated in the same builder, it will be considered deactivated.
    pub fn deactivate_feature(&mut self, group: &str, name: &str, value: &str) -> &mut Self {
        self.exclude_features.push(FeatureReference {
            group: group.to_owned(),
            name: name.to_owned(),
            value: value.to_owned(),
        });
        self
    }

    /// Determine whether a given parcel should be disabled according to the filter.
    fn is_disabled(&self, parcel: &Parcel) -> bool {
        match &parcel.label.feature {
            None => false,
            Some(feat) => {
                // If the exclude list comes up empty then this parcel is fine.
                // But if there are any matches, the parcel should be disabled.
                self.exclude_features
                    .iter()
                    .any(|key| match feat.get(&key.group) {
                        None => false,
                        Some(features) => {
                            features.get(&key.name).unwrap_or(&"".to_owned()) == &key.value
                        }
                    })
                // If the previous returned "false", then we want to see if the parcel
                // contains a feature that is supposed to be enabled. If so, then we make
                // sure that the key and value both match.
                ||
                self.features.iter().any(|key| match feat.get(&key.group) {
                        None => false, // Passes if there is no group
                        Some(features) => {
                            // if it has the key

                            // Passes if there is a key and it matches
                            !features.get(&key.name).map(|val| val == &key.value).unwrap_or(false)
                        }
                    })
            }
        }
    }

    // Do we filter media types, too?
    // Do we filter by size?
    pub fn filter(&self) -> Vec<Parcel> {
        // First we need to find all of the groups that should be enabled. These can be
        // enabled because of their 'required' flag or because they are in the the
        // 'groups' set on this struct. This is a special pass over groups because it
        // must take into account the 'required' flag. Subsequent passes do not.
        let mut groups: HashSet<String> = match &self.invoice.group {
            Some(group) => {
                group
                    .iter()
                    .filter(|&i| {
                        // Skip any group explicitly in the exclude list
                        // FIXME: Do we really want to allow this as an override to
                        // a required group?
                        if self.exclude_groups.contains(&i.name) {
                            return false;
                        }
                        i.required.unwrap_or(false) || self.groups.contains(&i.name)
                    })
                    .map(|g| g.name.clone())
                    .collect()
            }
            // If there are no groups, then our current list of parcels is complete.
            None => HashSet::new(),
        };

        // Build a list of parcels that are enabled by default, or enabled because a group
        // is enabled.
        let zero_vec = Vec::with_capacity(0);
        let mut parcels: HashSet<Parcel> = self
            .invoice
            .parcel
            .as_ref()
            .unwrap_or(&zero_vec)
            .iter()
            .filter(|p| {
                // Filter out any parcels that are not part of the global group or one
                // of the enabled groups.
                // If conditions is None or conditions.member_of is None, then this parcel
                // is a member of the global group.
                p.conditions
                    .as_ref()
                    .map(|c| {
                        match &c.member_of {
                            // In the global group
                            None => true,
                            // In an enabled group
                            Some(gnames) => gnames.iter().any(|n| groups.contains(n)),
                        }
                    })
                    .unwrap_or(true) // No conditions means parcel is in global group
            })
            .filter(|p| !self.is_disabled(p))
            .cloned()
            .collect();

        // Loop through the parcels and see if any of them require in more groups.
        // If so, descend down that tree and add parcels.
        let dependencies: HashSet<Parcel> = parcels.iter().fold(HashSet::new(), |mut deps, p| {
            if let Some(extras) = self.walk_parcel_requires(p, &mut groups) {
                deps.extend(extras)
            }
            deps
        });

        // Add all of the dependencies to the main parcel list.
        parcels.extend(dependencies);

        // Collect it into a Vec
        parcels.into_iter().collect()
    }

    /// Given a parcel, get a list of all of the parcels that are required via groups.
    ///
    /// A parcel can require zero or more groups. And a group can container zero or more
    /// parcels. This function takes a parcel and a list of already-resolved groups. Then
    /// for every group this parcel requires, it will find all the parcels that are members
    /// of that group and return them.
    ///
    /// This is recursive. If Parcel A requires Group 1, Group 1 requires Parcel B, and Parcel B
    /// requires Group 2, this function will walk down the tree and return a combined list
    /// of all of the parcels that are required to satisfy the top-level parcel's requirements.
    fn walk_parcel_requires(
        &self,
        p: &Parcel,
        groupset: &mut HashSet<String>,
    ) -> Option<HashSet<Parcel>> {
        // By passing in the group set, we should be able to prevent direct or indirect
        // infinite recursion, since all relationships are proxied through the a layer
        // of group indirection.

        let mut ret: HashSet<Parcel> = HashSet::new();
        // For each parcel in this set, see if it has required groups.
        if let Some(c) = p.conditions.as_ref() {
            if let Some(req) = &c.requires {
                // Loop through the list of required groups and add any that are not present.
                req.iter().for_each(|r| {
                    // If the groupset does not already have this group, then we can skip
                    // it.
                    if groupset.contains(r) {
                        return;
                    }
                    // Otherwise, we mark this group as processed before we recurse down
                    // to children. This prevents infinite recursion.
                    groupset.insert(r.to_owned());
                    // Loop through parcels to find ones in this group.
                    if let Some(pvec) = self.invoice.parcel.as_ref() {
                        pvec.iter().for_each(|p| {
                            // Loop through the member_of data and see if this
                            // parcel is a member of the r group.
                            if let Some(c) = p.conditions.as_ref() {
                                if let Some(groups) = &c.member_of {
                                    if groups.iter().any(|g| g == r) {
                                        // Check to see if this parcel should be disabled.
                                        // If so, skip it and all of its children.
                                        if self.is_disabled(p) {
                                            return;
                                        }

                                        ret.insert(p.clone());

                                        // Now recurse on this parcel
                                        let sub = self.walk_parcel_requires(p, groupset);
                                        if let Some(extras) = sub {
                                            ret.extend(extras)
                                        }
                                    }
                                }
                            }
                        });
                    }
                })
            }
        }
        if ret.is_empty() {
            return None;
        }
        Some(ret)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_global_group() {
        // Make sure that only modules in the global group are returned by default.
        let toml = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "test/is-disabled"
        version = "0.1.0"

        [[group]]
        name = "clownfish"

        [[group]]
        name = "unused"

        # Not in global
        [[parcel]]
        [parcel.label]
        name = "not_global"
        sha256 = "12345"
        mediaType = "application/octet-stream"
        size = 123
        [parcel.conditions]
        memberOf = ["clownfish"]

        # Not in global because an empty membership is non-global.
        [[parcel]]
        [parcel.label]
        name = "also_not_global"
        sha256 = "12345"
        mediaType = "application/octet-stream"
        size = 123
        [parcel.conditions]
        memberOf = [""]

        # This is in the global group
        [[parcel]]
        [parcel.label]
        name = "is_global"
        sha256 = "4321"
        mediaType = "application/octet-stream"
        size = 321
        [parcel.conditions]
        requires = ["unused"]

        # This is in the global group
        [[parcel]]
        [parcel.label]
        name = "also_is_global"
        sha256 = "4321"
        mediaType = "application/octet-stream"
        size = 321
        "#;

        let inv: crate::Invoice = toml::from_str(toml).expect("test invoice parsed");
        // If we leave everything on, we should get two bindles. The two should be members
        // of the global group.
        let filter = BindleFilter::new(&inv).filter();
        assert_eq!(2, filter.len());
    }

    #[test]
    fn test_deactivate_feature() {
        let toml = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "test/is-disabled"
        version = "0.1.0"

        [[parcel]]
        [parcel.label]
        name = "is_disabled"
        sha256 = "12345"
        mediaType = "application/octet-stream"
        size = 123
        [parcel.label.feature.testing]
        disabled = "true"

        [[parcel]]
        [parcel.label]
        name = "not_disabled"
        sha256 = "4321"
        mediaType = "application/octet-stream"
        size = 321

        "#;

        let inv: crate::Invoice = toml::from_str(toml).expect("test invoice parsed");
        // If we leave everything on, we should get two bindles.
        {
            let filter = BindleFilter::new(&inv).filter();
            assert_eq!(2, filter.len());
        }

        // If we disable "testing.disabled=true", this should be one.
        {
            let filter = BindleFilter::new(&inv)
                .deactivate_feature("testing", "disabled", "true")
                .filter();
            assert_eq!(1, filter.len());
        }

        // Verify that if a feature is activated and deactivated, deactivation wins.
        {
            let filter = BindleFilter::new(&inv)
                .deactivate_feature("testing", "disabled", "true")
                .activate_feature("testing", "disabled", "true")
                .filter();
            assert_eq!(1, filter.len());
        }

        // If we disable "testing.disabled=false", this should be two.
        {
            let filter = BindleFilter::new(&inv)
                .deactivate_feature("testing", "disabled", "false")
                .filter();
            assert_eq!(2, filter.len());
        }
    }

    #[test]
    fn test_activate_feature() {
        let toml = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "test/activate"
        version = "0.1.0"

        [[parcel]]
        [parcel.label]
        name = "narwhal_handler"
        sha256 = "12345"
        mediaType = "application/octet-stream"
        size = 123
        [parcel.label.feature.testing]
        animal = "narwhal"
        color = "blue"

        [[parcel]]
        [parcel.label]
        name = "unicorn_handler"
        sha256 = "4321"
        mediaType = "application/octet-stream"
        size = 321
        [parcel.label.feature.testing]
        animal = "unicorn"
        color = "blue"

        [[parcel]]
        [parcel.label]
        name = "default_thinger"
        sha256 = "5432"
        mediaType = "application/octet-stream"
        size = 321

        "#;

        let inv: crate::Invoice = toml::from_str(toml).expect("test invoice parsed");

        // By default, we should get all three bindles, since they are all in global.
        {
            let filter = BindleFilter::new(&inv).filter();
            assert_eq!(3, filter.len());
        }

        // If we enable testing/animal/narwhal, we should get only two.
        // The `unicorn_handler` does not meet our requirements
        {
            let filter = BindleFilter::new(&inv)
                .activate_feature("testing", "animal", "narwhal")
                .filter();
            assert_eq!(2, filter.len());

            // We need to make sure that we got the narwhal.
            assert!(filter.iter().any(|p| p.label.name == "narwhal_handler"));
            assert!(!filter.iter().any(|p| p.label.name == "unicorn_handler"));
        }

        // If we deactivate AND activate narwhal, we should get only the one default
        // module.
        {
            let filter = BindleFilter::new(&inv)
                .activate_feature("testing", "animal", "narwhal")
                .deactivate_feature("testing", "animal", "narwhal")
                .filter();
            assert_eq!(1, filter.len());
        }

        // If we activate both narwhal and unicorn, we should get only the last one
        // activated. So we should get a match for unicorn, but not narwhal
        {
            let filter = BindleFilter::new(&inv)
                .activate_feature("testing", "animal", "narwhal")
                .activate_feature("testing", "animal", "unicorn")
                .filter();
            assert_eq!(2, filter.len());
            assert!(filter.iter().any(|p| p.label.name == "unicorn_handler"));
        }
    }

    #[test]
    fn test_required_groups() {
        let toml = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "test/is-disabled"
        version = "0.1.0"

        [[group]]
        name = "is_required"
        required = true

        [[group]]
        name = "is_optional"

        [[group]]
        name = "also_optional"

        [[parcel]]
        [parcel.label]
        name = "first"
        sha256 = "12345"
        mediaType = "application/octet-stream"
        size = 123
        [parcel.conditions]
        memberOf = ["is_required"]

        [[parcel]]
        [parcel.label]
        name = "second"
        sha256 = "4321"
        mediaType = "application/octet-stream"
        size = 321
        [parcel.conditions]
        memberOf = ["is_optional", "also_optional"]

        [[parcel]]
        [parcel.label]
        name = "third"
        sha256 = "4321"
        mediaType = "application/octet-stream"
        size = 321
        [parcel.conditions]
        memberOf = ["also_optional"]
        "#;
        let inv: crate::Invoice = toml::from_str(toml).expect("test invoice parsed");

        // Check that by default we have one parcel, because group is required.
        {
            let filter = BindleFilter::new(&inv).filter();
            assert_eq!(1, filter.len());
        }

        // Activating one group should get us an additional parcel
        {
            let filter = BindleFilter::new(&inv).with_group("is_optional").filter();
            assert_eq!(2, filter.len());
        }

        // Activating two groups should should get us two additional parcels. But parcel
        // two should not be present twice (it is a member of two groups)
        {
            let filter = BindleFilter::new(&inv)
                .with_group("is_optional")
                .with_group("also_optional")
                .filter();
            assert_eq!(3, filter.len());
        }
    }

    #[test]
    fn test_circular_dependency() {
        let toml = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "test/is-disabled"
        version = "0.1.0"

        [[group]]
        name = "first"
        required = true

        [[group]]
        name = "second"

        [[group]]
        name = "third"

        [[parcel]]
        [parcel.label]
        name = "p1"
        sha256 = "12345"
        mediaType = "application/octet-stream"
        size = 123
        [parcel.conditions]
        memberOf = ["first"]
        requires = ["second"]

        [[parcel]]
        [parcel.label]
        name = "p2"
        sha256 = "4321"
        mediaType = "application/octet-stream"
        size = 321
        [parcel.conditions]
        memberOf = ["second"]
        requires =[ "third"]

        [[parcel]]
        [parcel.label]
        name = "p3"
        sha256 = "4321"
        mediaType = "application/octet-stream"
        size = 321
        [parcel.conditions]
        memberOf = ["third"]
        requires = ["first", "second"] # should not cause an infinite loop
        "#;
        let inv: crate::Invoice = toml::from_str(toml).expect("test invoice parsed");

        // Should have three. More importantly, should not get stuck in an infinite loop.
        let filter = BindleFilter::new(&inv).filter();
        assert_eq!(3, filter.len());
    }

    #[test]
    fn test_dependency_resolution() {
        let toml = r#"
        bindleVersion = "1.0.0"

        [bindle]
        name = "test/is-disabled"
        version = "0.1.0"

        [[group]]
        name = "first"
        required = true

        [[group]]
        name = "second"

        [[group]]
        name = "third"

        [[parcel]]
        [parcel.label]
        name = "p1"
        sha256 = "12345"
        mediaType = "application/octet-stream"
        size = 123
        [parcel.conditions]
        memberOf = ["first"]
        requires = ["second"]

        [[parcel]]
        [parcel.label]
        name = "p2"
        sha256 = "4321"
        mediaType = "application/octet-stream"
        size = 321
        [parcel.conditions]
        memberOf = ["second"]
        requires =[ "third"]

        [[parcel]]
        [parcel.label]
        name = "p3"
        sha256 = "4321"
        mediaType = "application/octet-stream"
        size = 321
        [parcel.conditions]
        memberOf = ["third"]
        "#;
        let inv: crate::Invoice = toml::from_str(toml).expect("test invoice parsed");

        // By default, we should get all three because "first" is required
        {
            let filter = BindleFilter::new(&inv).filter();
            assert_eq!(3, filter.len());
        }

        // Disabling "first" should disable all
        {
            let filter = BindleFilter::new(&inv).without_group("first").filter();
            assert_eq!(0, filter.len());
        }
    }

    // That's right, YAML-loving world, I can RECKLESSLY INDENT this stuff WITHOUT
    // causing the parser problems.
    const TEST_BINDLE_FILTERS_INVOICE: &str = r#"
    bindleVersion = "1.0.0"

    [bindle]
    name = "example/weather-progressive"
    version = "0.1.0"
    authors = ["Matt Butcher <matt.butcher@microsoft.com>"]
    description = "Weather Prediction"

    [[group]]
    name = "entrypoint"
    satisfiedBy = "oneOf"
    required = true

    [[group]]
    name = "ui-support"
    satisfiedBy = "allOf"
    required = false

    [[parcel]]
    [parcel.label]
    sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
    mediaType = "application/wasm"
    name = "weather-ui.wasm"
    size = 1710256
    [parcel.label.feature.wasm]
    ui-kit = "electron+sgu"
    [parcel.conditions]
    memberOf = ["entrypoint"]
    requires = ["ui-support"]

    [[parcel]]
    [parcel.label]
    sha256 = "048264cef43e4fead1701e48f3287d35386474cb"
    mediaType = "application/wasm"
    name = "weather-cli.wasm"
    size = 1410256
    [parcel.conditions]
    memberOf = ["entrypoint"]

    [[parcel]]
    [parcel.label]
    sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
    mediaType = "application/wasm"
    name = "libalmanac.wasm"
    size = 2561710
    [parcel.label.feature.wasm]
    type = "library"

    [[parcel]]
    [parcel.label]
    sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
    mediaType = "text/html"
    name = "almanac-ui.html"
    size = 2561710
    [parcel.label.feature.wasm]
    type = "data"
    [parcel.conditions]
    memberOf = ["ui-support"]

    [[parcel]]
    [parcel.label]
    sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
    mediaType = "text/css"
    name = "styles.css"
    size = 2561710
    [parcel.label.feature.wasm]
    type = "data"
    [parcel.conditions]
    memberOf = ["ui-support"]

    [[parcel]]
    [parcel.label]
    sha256 = "4cb048264cef43e4fead1701e48f3287d3538647"
    mediaType = "application/wasm"
    name = "uibuilder.wasm"
    size = 2561710
    [parcel.label.feature.wasm]
    type = "library"
    [parcel.conditions]
    memberOf = ["ui-support"]
    "#;

    #[test]
    fn test_bindle_filters() {
        let inv: crate::Invoice =
            toml::from_str(TEST_BINDLE_FILTERS_INVOICE).expect("test invoice parsed");

        // The parcels should be processed like this:
        // - One global parcel
        // - Two parcels included as "entrypoint"
        // - Three more parcels added by the `requires` directive on weather-ui.wasm
        {
            let filter = BindleFilter::new(&inv).filter();
            assert_eq!(6, filter.len());
        }

        // We can disable the "entrypoint" group, and then we should have only one group.
        {
            let filter = BindleFilter::new(&inv).without_group("entrypoint").filter();
            assert_eq!(1, filter.len());
        }
    }
}
