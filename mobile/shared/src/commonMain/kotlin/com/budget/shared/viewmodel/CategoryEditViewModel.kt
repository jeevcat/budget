package com.budget.shared.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.budget.shared.api.BudgetApi
import com.budget.shared.api.BudgetMode
import com.budget.shared.api.BudgetType
import com.budget.shared.api.Category
import com.budget.shared.api.CategoryRequest
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class CategoryEditUiState(
    val name: String = "",
    val parentId: String? = null,
    val budgetMode: BudgetMode? = null,
    val budgetType: BudgetType = BudgetType.VARIABLE,
    val budgetAmount: String = "",
    val projectStartDate: String = "",
    val projectEndDate: String = "",
    val availableParents: List<ParentOption> = emptyList(),
    val saving: Boolean = false,
    val saved: Boolean = false,
    val error: String? = null,
    val isEditing: Boolean = false,
)

data class ParentOption(
    val id: String,
    val name: String,
)

/** Abstraction for saving a category so the ViewModel is unit-testable. */
interface CategorySaver {
  suspend fun fetchCategories(serverUrl: String, apiKey: String): List<Category>

  suspend fun createCategory(serverUrl: String, apiKey: String, request: CategoryRequest): Category

  suspend fun updateCategory(
      serverUrl: String,
      apiKey: String,
      id: String,
      request: CategoryRequest,
  ): Category
}

class DefaultCategorySaver : CategorySaver {
  override suspend fun fetchCategories(serverUrl: String, apiKey: String): List<Category> {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      api.getCategories()
    } finally {
      api.close()
    }
  }

  override suspend fun createCategory(
      serverUrl: String,
      apiKey: String,
      request: CategoryRequest,
  ): Category {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      api.createCategory(request)
    } finally {
      api.close()
    }
  }

  override suspend fun updateCategory(
      serverUrl: String,
      apiKey: String,
      id: String,
      request: CategoryRequest,
  ): Category {
    val api = BudgetApi(serverUrl, apiKey)
    return try {
      api.updateCategory(id, request)
    } finally {
      api.close()
    }
  }
}

class CategoryEditViewModel(
    private val serverUrl: String,
    private val apiKey: String,
    private val editingCategory: Category? = null,
    private val saver: CategorySaver = DefaultCategorySaver(),
) : ViewModel() {

  private val _uiState = MutableStateFlow(CategoryEditUiState(isEditing = editingCategory != null))
  val uiState: StateFlow<CategoryEditUiState> = _uiState.asStateFlow()

  init {
    if (editingCategory != null) {
      _uiState.update {
        it.copy(
            name = editingCategory.name,
            parentId = editingCategory.parentId,
            budgetMode = editingCategory.budgetMode,
            budgetType = editingCategory.budgetType ?: BudgetType.VARIABLE,
            budgetAmount = editingCategory.budgetAmount ?: "",
            projectStartDate = editingCategory.projectStartDate ?: "",
            projectEndDate = editingCategory.projectEndDate ?: "",
        )
      }
    }
    loadParents()
  }

  private fun loadParents() {
    viewModelScope.launch {
      try {
        val categories = saver.fetchCategories(serverUrl, apiKey)
        val parents =
            categories
                .filter { it.parentId == null && it.id != editingCategory?.id }
                .sortedBy { it.name.lowercase() }
                .map { ParentOption(id = it.id, name = it.name) }
        _uiState.update { it.copy(availableParents = parents) }
      } catch (_: Exception) {
        // Non-critical — form still works without parent picker
      }
    }
  }

  fun updateName(name: String) {
    _uiState.update { it.copy(name = name, error = null) }
  }

  fun updateParentId(parentId: String?) {
    _uiState.update { it.copy(parentId = parentId, error = null) }
  }

  fun updateBudgetMode(mode: BudgetMode?) {
    _uiState.update { it.copy(budgetMode = mode, error = null) }
  }

  fun updateBudgetType(type: BudgetType) {
    _uiState.update { it.copy(budgetType = type, error = null) }
  }

  fun updateBudgetAmount(amount: String) {
    _uiState.update { it.copy(budgetAmount = amount, error = null) }
  }

  fun updateProjectStartDate(date: String) {
    _uiState.update { it.copy(projectStartDate = date, error = null) }
  }

  fun updateProjectEndDate(date: String) {
    _uiState.update { it.copy(projectEndDate = date, error = null) }
  }

  fun save() {
    val state = _uiState.value
    val trimmedName = state.name.trim()
    if (trimmedName.isEmpty()) {
      _uiState.update { it.copy(error = "Name is required") }
      return
    }

    _uiState.update { it.copy(saving = true, error = null) }
    viewModelScope.launch {
      try {
        val request = buildRequest(state, trimmedName)
        if (editingCategory != null) {
          saver.updateCategory(serverUrl, apiKey, editingCategory.id, request)
        } else {
          saver.createCategory(serverUrl, apiKey, request)
        }
        _uiState.update { it.copy(saving = false, saved = true) }
      } catch (e: Exception) {
        _uiState.update { it.copy(saving = false, error = e.message ?: "Failed to save") }
      }
    }
  }

  companion object {
    fun buildRequest(state: CategoryEditUiState, name: String): CategoryRequest {
      val budgetMode = state.budgetMode
      return CategoryRequest(
          name = name,
          parentId = state.parentId,
          budgetMode = budgetMode?.let { modeToString(it) },
          budgetType = if (budgetMode != null) typeToString(state.budgetType) else null,
          budgetAmount =
              if (budgetMode != null && state.budgetAmount.isNotBlank()) state.budgetAmount.trim()
              else null,
          projectStartDate =
              if (budgetMode == BudgetMode.PROJECT && state.projectStartDate.isNotBlank())
                  state.projectStartDate.trim()
              else null,
          projectEndDate =
              if (budgetMode == BudgetMode.PROJECT && state.projectEndDate.isNotBlank())
                  state.projectEndDate.trim()
              else null,
      )
    }

    fun modeToString(mode: BudgetMode): String =
        when (mode) {
          BudgetMode.MONTHLY -> "monthly"
          BudgetMode.ANNUAL -> "annual"
          BudgetMode.PROJECT -> "project"
        }

    fun typeToString(type: BudgetType): String =
        when (type) {
          BudgetType.FIXED -> "fixed"
          BudgetType.VARIABLE -> "variable"
        }
  }
}
