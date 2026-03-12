package com.budget.shared.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
enum class BudgetMode {
  @SerialName("monthly") MONTHLY,
  @SerialName("annual") ANNUAL,
  @SerialName("project") PROJECT,
  @SerialName("salary") SALARY,
  @SerialName("transfer") TRANSFER,
}

@Serializable
enum class BudgetType {
  @SerialName("fixed") FIXED,
  @SerialName("variable") VARIABLE,
}

@Serializable
enum class PaceIndicator {
  @SerialName("pending") PENDING,
  @SerialName("under_budget") UNDER_BUDGET,
  @SerialName("on_track") ON_TRACK,
  @SerialName("above_pace") ABOVE_PACE,
  @SerialName("over_budget") OVER_BUDGET,
}

@Serializable
data class BudgetMonth(
    val id: String,
    @SerialName("start_date") val startDate: String,
    @SerialName("end_date") val endDate: String? = null,
    @SerialName("salary_transactions_detected") val salaryTransactionsDetected: Int = 0,
)

@Serializable
data class ChildCategoryInfo(
    @SerialName("category_id") val categoryId: String,
    @SerialName("category_name") val categoryName: String,
)

@Serializable
data class BudgetGroupSummary(
    @SerialName("total_budget") val totalBudget: Double,
    @SerialName("total_spent") val totalSpent: Double,
    val remaining: Double,
    @SerialName("over_budget_count") val overBudgetCount: Int,
    @SerialName("bar_max") val barMax: Double,
)

@Serializable
data class CashFlowItem(
    @SerialName("category_id") val categoryId: String? = null,
    val label: String,
    val amount: Double,
    @SerialName("transaction_count") val transactionCount: Int,
    val transactions: List<TransactionEntry> = emptyList(),
)

@Serializable
data class LedgerSummary(
    val income: List<CashFlowItem> = emptyList(),
    @SerialName("total_in") val totalIn: Double,
    val unbudgeted: List<CashFlowItem> = emptyList(),
    @SerialName("total_out") val totalOut: Double,
    val net: Double,
    val saved: Double,
    @SerialName("bar_max") val barMax: Double,
)

@Serializable
data class BudgetStatus(
    @SerialName("category_id") val categoryId: String,
    @SerialName("category_name") val categoryName: String,
    @SerialName("budget_amount") val budgetAmount: Double,
    val spent: Double,
    val remaining: Double,
    @SerialName("time_left") val timeLeft: Long? = null,
    val pace: PaceIndicator,
    @SerialName("pace_delta") val paceDelta: Double = 0.0,
    @SerialName("budget_mode") val budgetMode: BudgetMode,
    val children: List<ChildCategoryInfo> = emptyList(),
    @SerialName("has_children") val hasChildren: Boolean = false,
)

@Serializable
data class ProjectChildSpending(
    @SerialName("category_id") val categoryId: String,
    @SerialName("category_name") val categoryName: String,
    val spent: Double,
)

@Serializable
data class ProjectStatusEntry(
    @SerialName("category_id") val categoryId: String,
    @SerialName("category_name") val categoryName: String,
    @SerialName("budget_amount") val budgetAmount: Double,
    val spent: Double,
    val remaining: Double,
    @SerialName("time_left") val timeLeft: Long? = null,
    val pace: PaceIndicator,
    @SerialName("pace_delta") val paceDelta: Double = 0.0,
    @SerialName("budget_mode") val budgetMode: BudgetMode,
    val children: List<ProjectChildSpending> = emptyList(),
    @SerialName("has_children") val hasChildren: Boolean = false,
    @SerialName("project_start_date") val projectStartDate: String = "",
    @SerialName("project_end_date") val projectEndDate: String? = null,
    val finished: Boolean = false,
)

@Serializable
data class TransactionEntry(
    val id: String,
    @SerialName("category_id") val categoryId: String? = null,
    val amount: Double,
    @SerialName("merchant_name") val merchantName: String = "",
    @SerialName("remittance_information") val remittanceInformation: List<String> = emptyList(),
    @SerialName("posted_date") val postedDate: String,
)

@Serializable
data class StatusResponse(
    val month: BudgetMonth,
    val statuses: List<BudgetStatus>,
    val projects: List<ProjectStatusEntry> = emptyList(),
    @SerialName("monthly_ledger") val monthlyLedger: LedgerSummary,
    @SerialName("annual_ledger") val annualLedger: LedgerSummary,
    @SerialName("project_summary") val projectSummary: BudgetGroupSummary,
    @SerialName("monthly_transactions")
    val monthlyTransactions: List<TransactionEntry> = emptyList(),
    @SerialName("annual_transactions") val annualTransactions: List<TransactionEntry> = emptyList(),
    @SerialName("project_transactions")
    val projectTransactions: List<TransactionEntry> = emptyList(),
    @SerialName("budget_year") val budgetYear: Int = 0,
)
