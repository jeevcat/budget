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
    val description: String = "",
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
