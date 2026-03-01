package com.budget.shared.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.budget.shared.api.BudgetApi
import com.budget.shared.api.Category
import com.budget.shared.api.CategoryMethod
import com.budget.shared.api.Transaction
import com.budget.shared.api.TransactionPage
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** A category with its resolved parent name for display. */
data class DisplayCategory(
    val id: String,
    val name: String,
    val parentName: String? = null,
    val depth: Int = 0,
) {
  val displayName: String
    get() = if (parentName != null) "$parentName > $name" else name
}

data class TransactionsUiState(
    val loading: Boolean = true,
    val error: String? = null,
    val transactions: List<Transaction> = emptyList(),
    val total: Int = 0,
    val categories: List<DisplayCategory> = emptyList(),
    /** The transaction currently open for category assignment. */
    val selectedTransaction: Transaction? = null,
    /** True while a categorize/uncategorize request is in flight. */
    val categorizing: Boolean = false,
    /** Search query for filtering the category picker. */
    val categorySearch: String = "",
)

/** Abstraction over API calls so the ViewModel is unit-testable. */
interface TransactionsFetcher {
  suspend fun fetchTransactions(
      serverUrl: String,
      apiKey: String,
      limit: Int,
      offset: Int,
      categoryId: String?,
  ): TransactionPage

  suspend fun fetchCategories(serverUrl: String, apiKey: String): List<Category>

  suspend fun categorize(
      serverUrl: String,
      apiKey: String,
      transactionId: String,
      categoryId: String,
  ): Boolean

  suspend fun uncategorize(
      serverUrl: String,
      apiKey: String,
      transactionId: String,
  ): Boolean
}

class DefaultTransactionsFetcher : TransactionsFetcher {
  override suspend fun fetchTransactions(
      serverUrl: String,
      apiKey: String,
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

  override suspend fun fetchCategories(serverUrl: String, apiKey: String): List<Category> {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      api.getCategories()
    } finally {
      api.close()
    }
  }

  override suspend fun categorize(
      serverUrl: String,
      apiKey: String,
      transactionId: String,
      categoryId: String,
  ): Boolean {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      api.categorizeTransaction(transactionId, categoryId)
    } finally {
      api.close()
    }
  }

  override suspend fun uncategorize(
      serverUrl: String,
      apiKey: String,
      transactionId: String,
  ): Boolean {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      api.uncategorizeTransaction(transactionId)
    } finally {
      api.close()
    }
  }
}

class TransactionsViewModel(
    private val serverUrl: String,
    private val apiKey: String,
    private val fetcher: TransactionsFetcher = DefaultTransactionsFetcher(),
) : ViewModel() {

  private val _uiState = MutableStateFlow(TransactionsUiState())
  val uiState: StateFlow<TransactionsUiState> = _uiState.asStateFlow()

  init {
    load()
  }

  fun refresh() {
    load()
  }

  fun selectTransaction(transaction: Transaction?) {
    _uiState.update { it.copy(selectedTransaction = transaction, categorySearch = "") }
  }

  fun updateCategorySearch(query: String) {
    _uiState.update { it.copy(categorySearch = query) }
  }

  /** Filtered categories based on the current search query. */
  fun filteredCategories(): List<DisplayCategory> {
    val state = _uiState.value
    val query = state.categorySearch.trim()
    if (query.isEmpty()) return state.categories
    return state.categories.filter { it.displayName.contains(query, ignoreCase = true) }
  }

  /** Assign a category to the currently selected transaction. */
  fun categorize(categoryId: String) {
    val txn = _uiState.value.selectedTransaction ?: return
    _uiState.update { it.copy(categorizing = true) }
    viewModelScope.launch {
      try {
        val success = fetcher.categorize(serverUrl, apiKey, txn.id, categoryId)
        if (success) {
          // Update the transaction in our local list
          _uiState.update { state ->
            val updated =
                state.transactions.map { t ->
                  if (t.id == txn.id)
                      t.copy(
                          categoryId = categoryId,
                          categoryMethod = CategoryMethod.MANUAL,
                      )
                  else t
                }
            // Remove from list since it's no longer uncategorized
            state.copy(
                categorizing = false,
                selectedTransaction = null,
                transactions = updated.filter { it.categoryId == null },
                total = (state.total - 1).coerceAtLeast(0),
            )
          }
        } else {
          _uiState.update {
            it.copy(
                categorizing = false,
                error = "Failed to categorize transaction",
            )
          }
        }
      } catch (e: Exception) {
        _uiState.update {
          it.copy(
              categorizing = false,
              error = e.message ?: "Unknown error",
          )
        }
      }
    }
  }

  /** Clear the category on the selected transaction (re-run auto-categorization). */
  fun uncategorize() {
    val txn = _uiState.value.selectedTransaction ?: return
    if (txn.categoryId == null) return
    _uiState.update { it.copy(categorizing = true) }
    viewModelScope.launch {
      try {
        val success = fetcher.uncategorize(serverUrl, apiKey, txn.id)
        if (success) {
          _uiState.update { state ->
            val updated =
                state.transactions.map { t ->
                  if (t.id == txn.id)
                      t.copy(
                          categoryId = null,
                          categoryMethod = null,
                      )
                  else t
                }
            state.copy(
                categorizing = false,
                selectedTransaction = null,
                transactions = updated,
            )
          }
        } else {
          _uiState.update {
            it.copy(
                categorizing = false,
                error = "Failed to uncategorize transaction",
            )
          }
        }
      } catch (e: Exception) {
        _uiState.update {
          it.copy(
              categorizing = false,
              error = e.message ?: "Unknown error",
          )
        }
      }
    }
  }

  /** Accept the AI-suggested category for the currently selected transaction. */
  fun acceptSuggestion() {
    val txn = _uiState.value.selectedTransaction ?: return
    val suggestion = txn.suggestedCategory ?: return
    val categories = _uiState.value.categories
    val match =
        categories.find { it.displayName.equals(suggestion, ignoreCase = true) }
            ?: categories.find { it.name.equals(suggestion, ignoreCase = true) }
            ?: return
    categorize(match.id)
  }

  fun clearError() {
    _uiState.update { it.copy(error = null) }
  }

  private fun load() {
    _uiState.update { it.copy(loading = true, error = null) }
    viewModelScope.launch {
      try {
        val page =
            fetcher.fetchTransactions(
                serverUrl,
                apiKey,
                limit = 200,
                offset = 0,
                categoryId = "__none",
            )
        val rawCategories = fetcher.fetchCategories(serverUrl, apiKey)
        val displayCategories = buildDisplayCategories(rawCategories)
        _uiState.update {
          it.copy(
              loading = false,
              transactions = page.items,
              total = page.total,
              categories = displayCategories,
          )
        }
      } catch (e: Exception) {
        _uiState.update { it.copy(loading = false, error = e.message ?: "Unknown error") }
      }
    }
  }

  companion object {
    /** Build a sorted, hierarchical display list from flat categories. */
    fun buildDisplayCategories(raw: List<Category>): List<DisplayCategory> {
      val byId = raw.associateBy { it.id }
      val roots = raw.filter { it.parentId == null }.sortedBy { it.name.lowercase() }
      val childrenOf =
          raw.filter { it.parentId != null }
              .groupBy { it.parentId }
              .mapValues { (_, v) -> v.sortedBy { it.name.lowercase() } }

      return buildList {
        for (root in roots) {
          add(DisplayCategory(id = root.id, name = root.name, depth = 0))
          val children = childrenOf[root.id].orEmpty()
          for (child in children) {
            add(
                DisplayCategory(
                    id = child.id,
                    name = child.name,
                    parentName = root.name,
                    depth = 1,
                )
            )
          }
        }
      }
    }
  }
}
