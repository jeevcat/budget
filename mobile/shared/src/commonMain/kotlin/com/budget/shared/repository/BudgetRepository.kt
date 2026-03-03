package com.budget.shared.repository

import com.budget.shared.api.BudgetMonth
import com.budget.shared.api.Category
import com.budget.shared.api.CategoryRequest
import com.budget.shared.api.StatusResponse
import com.budget.shared.api.Transaction
import com.budget.shared.api.TransactionPage
import kotlinx.coroutines.flow.SharedFlow

enum class InvalidationEvent {
  TRANSACTIONS,
  CATEGORIES,
}

data class DashboardData(
    val status: StatusResponse,
    val months: List<BudgetMonth>,
)

interface BudgetRepository {
  val invalidationEvents: SharedFlow<InvalidationEvent>

  suspend fun getDashboardData(monthId: String?): DashboardData

  suspend fun getTransactions(
      limit: Int,
      offset: Int,
      categoryId: String?,
  ): TransactionPage

  suspend fun getTransaction(id: String): Transaction

  suspend fun getCategories(): List<Category>

  suspend fun categorizeTransaction(transactionId: String, categoryId: String): Boolean

  suspend fun uncategorizeTransaction(transactionId: String): Boolean

  suspend fun createCategory(request: CategoryRequest): Category

  suspend fun updateCategory(id: String, request: CategoryRequest): Category
}
