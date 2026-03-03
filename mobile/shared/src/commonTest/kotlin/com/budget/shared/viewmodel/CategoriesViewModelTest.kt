package com.budget.shared.viewmodel

import com.budget.shared.api.BudgetMode
import com.budget.shared.api.Category
import com.budget.shared.api.CategoryName
import com.budget.shared.api.CategoryRequest
import com.budget.shared.api.Transaction
import com.budget.shared.api.TransactionPage
import com.budget.shared.repository.BudgetRepository
import com.budget.shared.repository.DashboardData
import com.budget.shared.repository.InvalidationEvent
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain

@OptIn(ExperimentalCoroutinesApi::class)
class CategoriesViewModelTest {

  private val testDispatcher = UnconfinedTestDispatcher()

  @BeforeTest
  fun setUp() {
    Dispatchers.setMain(testDispatcher)
  }

  @AfterTest
  fun tearDown() {
    Dispatchers.resetMain()
  }

  // -----------------------------------------------------------------------
  // ViewModel integration tests
  // -----------------------------------------------------------------------

  @Test
  fun initialLoadPopulatesState() = runTest {
    val repo = FakeCategoriesRepository(sampleCategories())
    val vm = CategoriesViewModel(repo)

    val state = vm.uiState.value
    assertFalse(state.loading)
    assertNull(state.error)
    assertTrue(state.treeItems.isNotEmpty())
  }

  @Test
  fun errorSetsErrorState() = runTest {
    val repo = FakeCategoriesRepository(error = "Network failure")
    val vm = CategoriesViewModel(repo)

    val state = vm.uiState.value
    assertFalse(state.loading)
    assertEquals("Network failure", state.error)
    assertTrue(state.treeItems.isEmpty())
  }

  @Test
  fun toggleParentCollapsesAndExpands() = runTest {
    val repo = FakeCategoriesRepository(sampleCategories())
    val vm = CategoriesViewModel(repo)

    // "Food" has a child, so it should be expanded by default
    assertTrue("food" in vm.uiState.value.expandedParents)

    vm.toggleParent("food")
    assertFalse("food" in vm.uiState.value.expandedParents)

    vm.toggleParent("food")
    assertTrue("food" in vm.uiState.value.expandedParents)
  }

  @Test
  fun allParentsExpandedByDefault() = runTest {
    val repo = FakeCategoriesRepository(sampleCategories())
    val vm = CategoriesViewModel(repo)

    val state = vm.uiState.value
    val parents = state.treeItems.filter { it.hasChildren }
    for (parent in parents) {
      assertTrue(
          parent.id in state.expandedParents,
          "Parent ${parent.name} should be expanded by default",
      )
    }
  }

  // -----------------------------------------------------------------------
  // buildTree tests
  // -----------------------------------------------------------------------

  @Test
  fun hierarchyPreservedRegardlessOfBudgetMode() = runTest {
    val tree = CategoriesViewModel.buildTree(sampleCategories())

    // "Dining Out" (unbudgeted child of Food) should appear directly after Food,
    // not in a separate section
    val names = tree.map { it.name }
    val foodIdx = names.indexOf("Food")
    val diningIdx = names.indexOf("Dining Out")
    assertTrue(diningIdx == foodIdx + 1, "Dining Out should be right after its parent Food")
  }

  @Test
  fun rootsSortedAlphabetically() {
    val tree = CategoriesViewModel.buildTree(sampleCategories())
    val roots = tree.filter { it.depth == 0 }
    assertEquals(
        roots.map { it.name },
        roots.sortedBy { it.name.lowercase() }.map { it.name },
    )
  }

  @Test
  fun childrenSortedAlphabeticallyUnderParent() {
    val categories =
        listOf(
            Category(id = "parent", name = catName("Parent"), transactionCount = 0),
            Category(
                id = "c",
                name = catName("Zulu"),
                parentId = "parent",
                transactionCount = 0,
            ),
            Category(
                id = "b",
                name = catName("Bravo"),
                parentId = "parent",
                transactionCount = 0,
            ),
            Category(
                id = "a",
                name = catName("Alpha"),
                parentId = "parent",
                transactionCount = 0,
            ),
        )
    val tree = CategoriesViewModel.buildTree(categories)
    val children = tree.filter { it.depth == 1 }
    assertEquals(listOf("Alpha", "Bravo", "Zulu"), children.map { it.name })
  }

  @Test
  fun threeLevelNestingProducesCorrectDepths() {
    val categories =
        listOf(
            Category(id = "house", name = catName("House"), transactionCount = 0),
            Category(
                id = "utilities",
                name = catName("Utilities"),
                parentId = "house",
                transactionCount = 0,
            ),
            Category(
                id = "electricity",
                name = catName("Electricity"),
                parentId = "utilities",
                transactionCount = 0,
            ),
        )
    val tree = CategoriesViewModel.buildTree(categories)

    assertEquals(3, tree.size)
    assertEquals(0, tree[0].depth) // House
    assertEquals("House", tree[0].name)
    assertEquals(1, tree[1].depth) // Utilities
    assertEquals("Utilities", tree[1].name)
    assertEquals(2, tree[2].depth) // Electricity
    assertEquals("Electricity", tree[2].name)
  }

  @Test
  fun hasChildrenFlagSetCorrectly() {
    val categories =
        listOf(
            Category(id = "parent", name = catName("Parent"), transactionCount = 0),
            Category(
                id = "child",
                name = catName("Child"),
                parentId = "parent",
                transactionCount = 0,
            ),
            Category(id = "leaf", name = catName("Leaf"), transactionCount = 0),
        )
    val tree = CategoriesViewModel.buildTree(categories)

    assertTrue(tree.first { it.id == "parent" }.hasChildren)
    assertFalse(tree.first { it.id == "child" }.hasChildren)
    assertFalse(tree.first { it.id == "leaf" }.hasChildren)
  }

  @Test
  fun budgetModePreservedOnEachItem() {
    val categories =
        listOf(
            Category(
                id = "house",
                name = catName("House"),
                transactionCount = 0,
            ),
            Category(
                id = "mortgage",
                name = catName("Mortgage"),
                parentId = "house",
                budgetMode = BudgetMode.MONTHLY,
                budgetAmount = "4000",
                transactionCount = 12,
            ),
            Category(
                id = "reno",
                name = catName("Renovation"),
                parentId = "house",
                budgetMode = BudgetMode.PROJECT,
                transactionCount = 3,
            ),
        )
    val tree = CategoriesViewModel.buildTree(categories)

    assertNull(tree.first { it.id == "house" }.budgetMode)
    assertEquals(BudgetMode.MONTHLY, tree.first { it.id == "mortgage" }.budgetMode)
    assertEquals(BudgetMode.PROJECT, tree.first { it.id == "reno" }.budgetMode)
  }

  // -----------------------------------------------------------------------
  // visibleItems tests
  // -----------------------------------------------------------------------

  @Test
  fun collapseParentHidesChildren() {
    val tree = CategoriesViewModel.buildTree(sampleCategories())
    // Collapse "Food" — "Dining Out" should disappear
    val expanded = tree.filter { it.hasChildren }.map { it.id }.toSet() - "food"
    val visible = CategoriesViewModel.visibleItems(tree, expanded)

    assertTrue(visible.any { it.id == "food" })
    assertFalse(visible.any { it.id == "dining" })
  }

  @Test
  fun expandParentShowsChildren() {
    val tree = CategoriesViewModel.buildTree(sampleCategories())
    val expanded = tree.filter { it.hasChildren }.map { it.id }.toSet()
    val visible = CategoriesViewModel.visibleItems(tree, expanded)

    assertTrue(visible.any { it.id == "food" })
    assertTrue(visible.any { it.id == "dining" })
  }

  @Test
  fun nestedCollapseHidesGrandchildren() {
    val categories =
        listOf(
            Category(id = "house", name = catName("House"), transactionCount = 0),
            Category(
                id = "utilities",
                name = catName("Utilities"),
                parentId = "house",
                transactionCount = 0,
            ),
            Category(
                id = "electricity",
                name = catName("Electricity"),
                parentId = "utilities",
                transactionCount = 0,
            ),
        )
    val tree = CategoriesViewModel.buildTree(categories)

    // Expand House but collapse Utilities
    val expanded = setOf("house")
    val visible = CategoriesViewModel.visibleItems(tree, expanded)

    assertEquals(2, visible.size)
    assertEquals("House", visible[0].name)
    assertEquals("Utilities", visible[1].name)
    assertFalse(visible.any { it.name == "Electricity" })
  }

  @Test
  fun collapseDoesNotAffectSiblings() {
    val categories =
        listOf(
            Category(id = "a", name = catName("Alpha"), transactionCount = 0),
            Category(
                id = "a1",
                name = catName("A-Child"),
                parentId = "a",
                transactionCount = 0,
            ),
            Category(id = "b", name = catName("Beta"), transactionCount = 0),
        )
    val tree = CategoriesViewModel.buildTree(categories)

    // Collapse Alpha — Beta should still be visible
    val visible = CategoriesViewModel.visibleItems(tree, emptySet())
    assertTrue(visible.any { it.name == "Alpha" })
    assertFalse(visible.any { it.name == "A-Child" })
    assertTrue(visible.any { it.name == "Beta" })
  }

  // -----------------------------------------------------------------------
  // leafName helper tests
  // -----------------------------------------------------------------------

  @Test
  fun leafNameReturnsNameWhenNoPrefix() {
    assertEquals("Groceries", CategoriesViewModel.leafName("Groceries", "Food"))
  }

  @Test
  fun leafNameReturnsNameWhenNoParent() {
    assertEquals("Cash", CategoriesViewModel.leafName("Cash", null))
  }

  @Test
  fun leafNameCategoryNameOverload() {
    assertEquals("Groceries", CategoriesViewModel.leafName(catName("Groceries"), "Food"))
  }

  @Test
  fun leafNameStripsColonPrefix() {
    assertEquals("Groceries", CategoriesViewModel.leafName("Food:Groceries", "Food"))
  }
}

// -- Helpers ----------------------------------------------------------------

/** Shorthand for constructing a [CategoryName] in tests where the name is known-valid. */
private fun catName(name: String): CategoryName = CategoryName.of(name).getOrThrow()

// -- Test doubles -----------------------------------------------------------

private class FakeCategoriesRepository(
    private val categories: List<Category> = emptyList(),
    private val error: String? = null,
) : BudgetRepository {

  private val _invalidationEvents = MutableSharedFlow<InvalidationEvent>()
  override val invalidationEvents: SharedFlow<InvalidationEvent> =
      _invalidationEvents.asSharedFlow()

  override suspend fun getDashboardData(monthId: String?): DashboardData =
      throw NotImplementedError()

  override suspend fun getTransactions(limit: Int, offset: Int, categoryId: String?) =
      TransactionPage(emptyList(), 0, limit, offset)

  override suspend fun getTransaction(id: String): Transaction = throw NotImplementedError()

  override suspend fun getCategories(): List<Category> {
    if (error != null) throw RuntimeException(error)
    return categories
  }

  override suspend fun categorizeTransaction(transactionId: String, categoryId: String) = true

  override suspend fun uncategorizeTransaction(transactionId: String) = true

  override suspend fun createCategory(request: CategoryRequest): Category =
      throw NotImplementedError()

  override suspend fun updateCategory(id: String, request: CategoryRequest): Category =
      throw NotImplementedError()
}

private fun sampleCategories(): List<Category> =
    listOf(
        Category(
            id = "food",
            name = catName("Food"),
            budgetMode = BudgetMode.MONTHLY,
            budgetAmount = "400",
            transactionCount = 20,
        ),
        Category(
            id = "dining",
            name = catName("Dining Out"),
            parentId = "food",
            budgetAmount = "100",
            transactionCount = 8,
        ),
        Category(
            id = "groceries",
            name = catName("Groceries"),
            budgetMode = BudgetMode.MONTHLY,
            budgetAmount = "300",
            transactionCount = 15,
        ),
        Category(
            id = "insurance",
            name = catName("Insurance"),
            budgetMode = BudgetMode.ANNUAL,
            budgetAmount = "1200",
            transactionCount = 4,
        ),
        Category(id = "misc", name = catName("Miscellaneous"), transactionCount = 3),
    )
