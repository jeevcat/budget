package com.budget.shared.viewmodel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.budget.shared.api.BudgetMode
import com.budget.shared.api.Category
import com.budget.shared.repository.BudgetRepository
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** A category prepared for display within a hierarchy tree. */
data class CategoryTreeItem(
    val id: String,
    val name: String,
    val budgetMode: BudgetMode? = null,
    val budgetAmount: String? = null,
    val transactionCount: Int = 0,
    val depth: Int = 0,
    val hasChildren: Boolean = false,
)

data class CategoriesUiState(
    val loading: Boolean = true,
    val error: String? = null,
    val treeItems: List<CategoryTreeItem> = emptyList(),
    val expandedParents: Set<String> = emptySet(),
    val categories: List<Category> = emptyList(),
)

class CategoriesViewModel(
    private val repository: BudgetRepository,
) : ViewModel() {

  private val _uiState = MutableStateFlow(CategoriesUiState())
  val uiState: StateFlow<CategoriesUiState> = _uiState.asStateFlow()

  init {
    refresh()
    viewModelScope.launch { repository.invalidationEvents.collect { refresh() } }
  }

  fun refresh() {
    _uiState.update { it.copy(loading = true, error = null) }
    viewModelScope.launch {
      try {
        val categories = repository.getCategories()
        val tree = buildTree(categories)
        val parentIds = tree.filter { it.hasChildren }.map { it.id }.toSet()
        _uiState.update {
          it.copy(
              loading = false,
              treeItems = tree,
              expandedParents = parentIds,
              categories = categories,
          )
        }
      } catch (e: Exception) {
        _uiState.update { it.copy(loading = false, error = e.message ?: "Unknown error") }
      }
    }
  }

  fun toggleParent(id: String) {
    _uiState.update { state ->
      val expanded =
          if (id in state.expandedParents) {
            state.expandedParents - id
          } else {
            state.expandedParents + id
          }
      state.copy(expandedParents = expanded)
    }
  }

  companion object {
    /**
     * Extract the leaf (display) name from a category name, stripping the parent prefix if present.
     *
     * Handles both naming conventions:
     * - `"Groceries"` under parent `"Food"` → `"Groceries"`
     * - `"Food:Groceries"` under parent `"Food"` → `"Groceries"`
     */
    fun leafName(name: String, parentName: String?): String {
      if (parentName != null) {
        val prefix = "$parentName:"
        if (name.startsWith(prefix)) {
          return name.removePrefix(prefix)
        }
      }
      return name
    }

    /** Convenience overload accepting a [CategoryName]. */
    fun leafName(name: com.budget.shared.api.CategoryName, parentName: String?): String =
        leafName(name.value, parentName)

    /**
     * Build a flat list of [CategoryTreeItem] in pre-order (parent before children). Root
     * categories (no parent) are sorted alphabetically, and children are sorted alphabetically
     * under each parent. Each item carries its depth (0, 1, 2).
     */
    fun buildTree(categories: List<Category>): List<CategoryTreeItem> {
      val byId = categories.associateBy { it.id }
      val childrenOf =
          categories
              .filter { it.parentId != null }
              .groupBy { it.parentId }
              .mapValues { (_, v) -> v.sortedBy { it.name.value.lowercase() } }

      val roots = categories.filter { it.parentId == null }.sortedBy { it.name.value.lowercase() }

      return buildList {
        fun visit(cat: Category, depth: Int) {
          val parentName = cat.parentId?.let { byId[it]?.name?.value }
          val displayName = leafName(cat.name, parentName)
          val children = childrenOf[cat.id].orEmpty()
          add(
              CategoryTreeItem(
                  id = cat.id,
                  name = displayName,
                  budgetMode = cat.budgetMode,
                  budgetAmount = cat.budgetAmount,
                  transactionCount = cat.transactionCount,
                  depth = depth,
                  hasChildren = children.isNotEmpty(),
              )
          )
          for (child in children) {
            visit(child, depth + 1)
          }
        }
        for (root in roots) {
          visit(root, 0)
        }
      }
    }

    /**
     * Filter [treeItems] to only include visible items based on [expandedParents]. When a parent is
     * collapsed, all deeper descendants are hidden until a sibling at the same or lesser depth is
     * reached.
     */
    fun visibleItems(
        treeItems: List<CategoryTreeItem>,
        expandedParents: Set<String>,
    ): List<CategoryTreeItem> = buildList {
      var skipUntilDepth = Int.MAX_VALUE
      for (item in treeItems) {
        if (item.depth > skipUntilDepth) continue
        skipUntilDepth = Int.MAX_VALUE
        add(item)
        if (item.hasChildren && item.id !in expandedParents) {
          skipUntilDepth = item.depth
        }
      }
    }
  }
}
