package com.budget.shared.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
enum class BudgetMode {
  @SerialName("monthly") MONTHLY,
  @SerialName("annual") ANNUAL,
  @SerialName("project") PROJECT,
}

@Serializable
enum class PaceIndicator {
  @SerialName("under_budget") UNDER_BUDGET,
  @SerialName("on_target") ON_TARGET,
  @SerialName("on_track") ON_TRACK,
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
data class BudgetStatus(
    @SerialName("category_id") val categoryId: String,
    @SerialName("category_name") val categoryName: String,
    @SerialName("budget_amount") val budgetAmount: Double,
    val spent: Double,
    val remaining: Double,
    @SerialName("time_left") val timeLeft: Long,
    val pace: PaceIndicator,
    @SerialName("pace_delta") val paceDelta: Double = 0.0,
    @SerialName("budget_mode") val budgetMode: BudgetMode,
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
    @SerialName("time_left") val timeLeft: Long,
    val pace: PaceIndicator,
    @SerialName("pace_delta") val paceDelta: Double = 0.0,
    @SerialName("budget_mode") val budgetMode: BudgetMode,
    val children: List<ProjectChildSpending> = emptyList(),
    @SerialName("has_children") val hasChildren: Boolean = false,
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
    @SerialName("monthly_transactions")
    val monthlyTransactions: List<TransactionEntry> = emptyList(),
    @SerialName("annual_transactions") val annualTransactions: List<TransactionEntry> = emptyList(),
    @SerialName("project_transactions")
    val projectTransactions: List<TransactionEntry> = emptyList(),
    @SerialName("uncategorized_count") val uncategorizedCount: Int = 0,
    @SerialName("budget_year") val budgetYear: Int = 0,
)
