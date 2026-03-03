package com.budget.shared.repository

import com.budget.shared.api.BudgetApi
import com.budget.shared.api.Category
import com.budget.shared.api.CategoryRequest
import com.budget.shared.api.Transaction
import com.budget.shared.api.TransactionPage
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow

class DefaultBudgetRepository(
    private val serverUrl: String,
    private val apiKey: String,
) : BudgetRepository {

  private val _invalidationEvents = MutableSharedFlow<InvalidationEvent>(extraBufferCapacity = 1)
  override val invalidationEvents: SharedFlow<InvalidationEvent> =
      _invalidationEvents.asSharedFlow()

  override suspend fun getDashboardData(monthId: String?): DashboardData {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      val status = api.getStatus(monthId)
      val months = api.getMonths()
      DashboardData(status, months)
    } finally {
      api.close()
    }
  }

  override suspend fun getTransactions(
      limit: Int,
      offset: Int,
      categoryId: String?,
  ): TransactionPage {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      api.getTransactions(limit = limit, offset = offset, categoryId = categoryId)
    } finally {
      api.close()
    }
  }

  override suspend fun getTransaction(id: String): Transaction {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      api.getTransaction(id)
    } finally {
      api.close()
    }
  }

  override suspend fun getCategories(): List<Category> {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      api.getCategories()
    } finally {
      api.close()
    }
  }

  override suspend fun categorizeTransaction(
      transactionId: String,
      categoryId: String,
  ): Boolean {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      val success = api.categorizeTransaction(transactionId, categoryId)
      if (success) _invalidationEvents.tryEmit(InvalidationEvent.TRANSACTIONS)
      success
    } finally {
      api.close()
    }
  }

  override suspend fun uncategorizeTransaction(transactionId: String): Boolean {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      val success = api.uncategorizeTransaction(transactionId)
      if (success) _invalidationEvents.tryEmit(InvalidationEvent.TRANSACTIONS)
      success
    } finally {
      api.close()
    }
  }

  override suspend fun createCategory(request: CategoryRequest): Category {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      val category = api.createCategory(request)
      _invalidationEvents.tryEmit(InvalidationEvent.CATEGORIES)
      category
    } finally {
      api.close()
    }
  }

  override suspend fun updateCategory(id: String, request: CategoryRequest): Category {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      val category = api.updateCategory(id, request)
      _invalidationEvents.tryEmit(InvalidationEvent.CATEGORIES)
      category
    } finally {
      api.close()
    }
  }
}
