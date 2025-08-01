// Code generated by software.amazon.smithy.rust.codegen.smithy-rs. DO NOT EDIT.
pub use crate::operation::generate_assistant_response::_generate_assistant_response_input::GenerateAssistantResponseInputBuilder;
pub use crate::operation::generate_assistant_response::_generate_assistant_response_output::GenerateAssistantResponseOutputBuilder;

impl crate::operation::generate_assistant_response::builders::GenerateAssistantResponseInputBuilder {
    /// Sends a request with this input using the given client.
    pub async fn send_with(
        self,
        client: &crate::Client,
    ) -> ::std::result::Result<
        crate::operation::generate_assistant_response::GenerateAssistantResponseOutput,
        ::aws_smithy_runtime_api::client::result::SdkError<
            crate::operation::generate_assistant_response::GenerateAssistantResponseError,
            ::aws_smithy_runtime_api::client::orchestrator::HttpResponse,
        >,
    > {
        let mut fluent_builder = client.generate_assistant_response();
        fluent_builder.inner = self;
        fluent_builder.send().await
    }
}
/// Fluent builder constructing a request to `GenerateAssistantResponse`.
///
/// API to generate assistant response.
#[derive(::std::clone::Clone, ::std::fmt::Debug)]
pub struct GenerateAssistantResponseFluentBuilder {
    handle: ::std::sync::Arc<crate::client::Handle>,
    inner: crate::operation::generate_assistant_response::builders::GenerateAssistantResponseInputBuilder,
    config_override: ::std::option::Option<crate::config::Builder>,
}
impl
    crate::client::customize::internal::CustomizableSend<
        crate::operation::generate_assistant_response::GenerateAssistantResponseOutput,
        crate::operation::generate_assistant_response::GenerateAssistantResponseError,
    > for GenerateAssistantResponseFluentBuilder
{
    fn send(
        self,
        config_override: crate::config::Builder,
    ) -> crate::client::customize::internal::BoxFuture<
        crate::client::customize::internal::SendResult<
            crate::operation::generate_assistant_response::GenerateAssistantResponseOutput,
            crate::operation::generate_assistant_response::GenerateAssistantResponseError,
        >,
    > {
        ::std::boxed::Box::pin(async move { self.config_override(config_override).send().await })
    }
}
impl GenerateAssistantResponseFluentBuilder {
    /// Creates a new `GenerateAssistantResponseFluentBuilder`.
    pub(crate) fn new(handle: ::std::sync::Arc<crate::client::Handle>) -> Self {
        Self {
            handle,
            inner: ::std::default::Default::default(),
            config_override: ::std::option::Option::None,
        }
    }

    /// Access the GenerateAssistantResponse as a reference.
    pub fn as_input(
        &self,
    ) -> &crate::operation::generate_assistant_response::builders::GenerateAssistantResponseInputBuilder {
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
        crate::operation::generate_assistant_response::GenerateAssistantResponseOutput,
        ::aws_smithy_runtime_api::client::result::SdkError<
            crate::operation::generate_assistant_response::GenerateAssistantResponseError,
            ::aws_smithy_runtime_api::client::orchestrator::HttpResponse,
        >,
    > {
        let input = self
            .inner
            .build()
            .map_err(::aws_smithy_runtime_api::client::result::SdkError::construction_failure)?;
        let runtime_plugins =
            crate::operation::generate_assistant_response::GenerateAssistantResponse::operation_runtime_plugins(
                self.handle.runtime_plugins.clone(),
                &self.handle.conf,
                self.config_override,
            );
        crate::operation::generate_assistant_response::GenerateAssistantResponse::orchestrate(&runtime_plugins, input)
            .await
    }

    /// Consumes this builder, creating a customizable operation that can be modified before being
    /// sent.
    pub fn customize(
        self,
    ) -> crate::client::customize::CustomizableOperation<
        crate::operation::generate_assistant_response::GenerateAssistantResponseOutput,
        crate::operation::generate_assistant_response::GenerateAssistantResponseError,
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

    /// Structure to represent the current state of a chat conversation.
    pub fn conversation_state(mut self, input: crate::types::ConversationState) -> Self {
        self.inner = self.inner.conversation_state(input);
        self
    }

    /// Structure to represent the current state of a chat conversation.
    pub fn set_conversation_state(mut self, input: ::std::option::Option<crate::types::ConversationState>) -> Self {
        self.inner = self.inner.set_conversation_state(input);
        self
    }

    /// Structure to represent the current state of a chat conversation.
    pub fn get_conversation_state(&self) -> &::std::option::Option<crate::types::ConversationState> {
        self.inner.get_conversation_state()
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn profile_arn(mut self, input: impl ::std::convert::Into<::std::string::String>) -> Self {
        self.inner = self.inner.profile_arn(input.into());
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn set_profile_arn(mut self, input: ::std::option::Option<::std::string::String>) -> Self {
        self.inner = self.inner.set_profile_arn(input);
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn get_profile_arn(&self) -> &::std::option::Option<::std::string::String> {
        self.inner.get_profile_arn()
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn agent_mode(mut self, input: impl ::std::convert::Into<::std::string::String>) -> Self {
        self.inner = self.inner.agent_mode(input.into());
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn set_agent_mode(mut self, input: ::std::option::Option<::std::string::String>) -> Self {
        self.inner = self.inner.set_agent_mode(input);
        self
    }

    #[allow(missing_docs)] // documentation missing in model
    pub fn get_agent_mode(&self) -> &::std::option::Option<::std::string::String> {
        self.inner.get_agent_mode()
    }
}
