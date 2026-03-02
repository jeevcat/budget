package com.budget.shared.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
enum class CategoryMethod {
  @SerialName("manual") MANUAL,
  @SerialName("rule") RULE,
  @SerialName("llm") LLM,
}

@Serializable
data class Transaction(
    val id: String,
    @SerialName("account_id") val accountId: String,
    @SerialName("category_id") val categoryId: String? = null,
    val amount: String,
    @SerialName("original_amount") val originalAmount: String? = null,
    @SerialName("original_currency") val originalCurrency: String? = null,
    @SerialName("merchant_name") val merchantName: String = "",
    @SerialName("remittance_information") val remittanceInformation: List<String> = emptyList(),
    @SerialName("merchant_category_code") val merchantCategoryCode: String? = null,
    @SerialName("bank_transaction_code_code") val bankTransactionCodeCode: String? = null,
    @SerialName("bank_transaction_code_sub_code") val bankTransactionCodeSubCode: String? = null,
    @SerialName("exchange_rate") val exchangeRate: String? = null,
    @SerialName("exchange_rate_unit_currency") val exchangeRateUnitCurrency: String? = null,
    @SerialName("exchange_rate_type") val exchangeRateType: String? = null,
    @SerialName("exchange_rate_contract_id") val exchangeRateContractId: String? = null,
    @SerialName("reference_number") val referenceNumber: String? = null,
    @SerialName("reference_number_schema") val referenceNumberSchema: String? = null,
    val note: String? = null,
    @SerialName("balance_after_transaction") val balanceAfterTransaction: String? = null,
    @SerialName("balance_after_transaction_currency")
    val balanceAfterTransactionCurrency: String? = null,
    @SerialName("posted_date") val postedDate: String,
    @SerialName("correlation_type") val correlationType: String? = null,
    @SerialName("category_method") val categoryMethod: CategoryMethod? = null,
    @SerialName("suggested_category") val suggestedCategory: String? = null,
    @SerialName("counterparty_name") val counterpartyName: String? = null,
    @SerialName("counterparty_iban") val counterpartyIban: String? = null,
    @SerialName("counterparty_bic") val counterpartyBic: String? = null,
    @SerialName("bank_transaction_code") val bankTransactionCode: String? = null,
    @SerialName("llm_justification") val llmJustification: String? = null,
    @SerialName("skip_correlation") val skipCorrelation: Boolean = false,
)

@Serializable
data class TransactionPage(
    val items: List<Transaction>,
    val total: Int,
    val limit: Int,
    val offset: Int,
)

@Serializable
data class Category(
    val id: String,
    val name: String,
    @SerialName("parent_id") val parentId: String? = null,
    @SerialName("budget_mode") val budgetMode: BudgetMode? = null,
    @SerialName("budget_amount") val budgetAmount: String? = null,
    @SerialName("project_start_date") val projectStartDate: String? = null,
    @SerialName("project_end_date") val projectEndDate: String? = null,
    @SerialName("transaction_count") val transactionCount: Int = 0,
)

@Serializable
data class CategorizeRequest(
    @SerialName("category_id") val categoryId: String,
)

@Serializable
data class CategoryRequest(
    val name: String,
    @SerialName("parent_id") val parentId: String? = null,
    @SerialName("budget_mode") val budgetMode: String? = null,
    @SerialName("budget_amount") val budgetAmount: String? = null,
    @SerialName("project_start_date") val projectStartDate: String? = null,
    @SerialName("project_end_date") val projectEndDate: String? = null,
)
