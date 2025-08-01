// Code generated by software.amazon.smithy.rust.codegen.smithy-rs. DO NOT EDIT.
pub use crate::operation::update_usage_limits::_update_usage_limits_input::UpdateUsageLimitsInputBuilder;
pub use crate::operation::update_usage_limits::_update_usage_limits_output::UpdateUsageLimitsOutputBuilder;

impl crate::operation::update_usage_limits::builders::UpdateUsageLimitsInputBuilder {
    /// Sends a request with this input using the given client.
    pub async fn send_with(
        self,
        client: &crate::Client,
    ) -> ::std::result::Result<
        crate::operation::update_usage_limits::UpdateUsageLimitsOutput,
        ::aws_smithy_runtime_api::client::result::SdkError<
            crate::operation::update_usage_limits::UpdateUsageLimitsError,
            ::aws_smithy_runtime_api::client::orchestrator::HttpResponse,
        >,
    > {
        let mut fluent_builder = client.update_usage_limits();
        fluent_builder.inner = self;
        fluent_builder.send().await
    }
}
/// Fluent builder constructing a request to `UpdateUsageLimits`.
///
/// API to update usage limits for enterprise customers
#[derive(::std::clone::Clone, ::std::fmt::Debug)]
pub struct UpdateUsageLimitsFluentBuilder {
    handle: ::std::sync::Arc<crate::client::Handle>,
    inner: crate::operation::update_usage_limits::builders::UpdateUsageLimitsInputBuilder,
    config_override: ::std::option::Option<crate::config::Builder>,
}
impl
    crate::client::customize::internal::CustomizableSend<
        crate::operation::update_usage_limits::UpdateUsageLimitsOutput,
        crate::operation::update_usage_limits::UpdateUsageLimitsError,
    > for UpdateUsageLimitsFluentBuilder
{
    fn send(
        self,
        config_override: crate::config::Builder,
    ) -> crate::client::customize::internal::BoxFuture<
        crate::client::customize::internal::SendResult<
            crate::operation::update_usage_limits::UpdateUsageLimitsOutput,
            crate::operation::update_usage_limits::UpdateUsageLimitsError,
        >,
    > {
        ::std::boxed::Box::pin(async move { self.config_override(config_override).send().await })
    }
}
impl UpdateUsageLimitsFluentBuilder {
    /// Creates a new `UpdateUsageLimitsFluentBuilder`.
    pub(crate) fn new(handle: ::std::sync::Arc<crate::client::Handle>) -> Self {
        Self {
            handle,
            inner: ::std::default::Default::default(),
            config_override: ::std::option::Option::None,
        }
    }

    /// Access the UpdateUsageLimits as a reference.
    pub fn as_input(&self) -> &crate::operation::update_usage_limits::builders::UpdateUsageLimitsInputBuilder {
        &self.inner
    }

    /// Sends the request and returns the response.
    ///
    /// If an error occurs, an `SdkError` will be returned with additional details that
    /// can be matched against.
    ///
    /// By default, any retryable failures will be retried twice. Retry behavior
    /// is configurable with the [RetryConfig](aws_smithy_types::retry::RetryConfig), which can be
    /// set when configuring the client.
    pub async fn send(
        self,
    ) -> ::std::result::Result<
        crate::operation::update_usage_limits::UpdateUsageLimitsOutput,
        ::aws_smithy_runtime_api::client::result::SdkError<
            crate::operation::update_usage_limits::UpdateUsageLimitsError,
            ::aws_smithy_runtime_api::client::orchestrator::HttpResponse,
        >,
    > {
        let input = self
            .inner
            .build()
            .map_err(::aws_smithy_runtime_api::client::result::SdkError::construction_failure)?;
        let runtime_plugins = crate::operation::update_usage_limits::UpdateUsageLimits::operation_runtime_plugins(
            self.handle.runtime_plugins.clone(),
            &self.handle.conf,
            self.config_override,
        );
        crate::operation::update_usage_limits::UpdateUsageLimits::orchestrate(&runtime_plugins, input).await
    }

    /// Consumes this builder, creating a customizable operation that can be modified before being
    /// sent.
    pub fn customize(
        self,
    ) -> crate::client::customize::CustomizableOperation<
        crate::operation::update_usage_limits::UpdateUsageLimitsOutput,
        crate::operation::update_usage_limits::UpdateUsageLimitsError,
        Self,
    > {
        crate::client::customize::CustomizableOperation::new(self)
    }

    pub(crate) fn config_override(
        mut self,
        config_override: impl ::std::convert::Into<crate::config::Builder>,
    ) -> Self {
        self.set_config_override(::std::option::Option::Some(config_override.into()));
        self
    }

    pub(crate) fn set_config_override(
        &mut self,
        config_override: ::std::option::Option<crate::config::Builder>,
    ) -> &mut Self {
        self.config_override = config_override;
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn account_id(mut self, input: impl ::std::convert::Into<::std::string::String>) -> Self {
        self.inner = self.inner.account_id(input.into());
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn set_account_id(mut self, input: ::std::option::Option<::std::string::String>) -> Self {
        self.inner = self.inner.set_account_id(input);
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn get_account_id(&self) -> &::std::option::Option<::std::string::String> {
        self.inner.get_account_id()
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn accountless_user_id(mut self, input: impl ::std::convert::Into<::std::string::String>) -> Self {
        self.inner = self.inner.accountless_user_id(input.into());
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn set_accountless_user_id(mut self, input: ::std::option::Option<::std::string::String>) -> Self {
        self.inner = self.inner.set_accountless_user_id(input);
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn get_accountless_user_id(&self) -> &::std::option::Option<::std::string::String> {
        self.inner.get_accountless_user_id()
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn directory_id(mut self, input: impl ::std::convert::Into<::std::string::String>) -> Self {
        self.inner = self.inner.directory_id(input.into());
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn set_directory_id(mut self, input: ::std::option::Option<::std::string::String>) -> Self {
        self.inner = self.inner.set_directory_id(input);
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn get_directory_id(&self) -> &::std::option::Option<::std::string::String> {
        self.inner.get_directory_id()
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn feature_type(mut self, input: crate::types::UsageLimitType) -> Self {
        self.inner = self.inner.feature_type(input);
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn set_feature_type(mut self, input: ::std::option::Option<crate::types::UsageLimitType>) -> Self {
        self.inner = self.inner.set_feature_type(input);
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn get_feature_type(&self) -> &::std::option::Option<crate::types::UsageLimitType> {
        self.inner.get_feature_type()
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn requested_limit(mut self, input: i64) -> Self {
        self.inner = self.inner.requested_limit(input);
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn set_requested_limit(mut self, input: ::std::option::Option<i64>) -> Self {
        self.inner = self.inner.set_requested_limit(input);
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn get_requested_limit(&self) -> &::std::option::Option<i64> {
        self.inner.get_requested_limit()
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn justification(mut self, input: impl ::std::convert::Into<::std::string::String>) -> Self {
        self.inner = self.inner.justification(input.into());
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn set_justification(mut self, input: ::std::option::Option<::std::string::String>) -> Self {
        self.inner = self.inner.set_justification(input);
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn get_justification(&self) -> &::std::option::Option<::std::string::String> {
        self.inner.get_justification()
    }
}
