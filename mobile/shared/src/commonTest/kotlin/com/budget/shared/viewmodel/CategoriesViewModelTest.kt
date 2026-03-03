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

  @Test
  fun initialLoadPopulatesState() = runTest {
    val repo = FakeCategoriesRepository(sampleCategories())
    val vm = CategoriesViewModel(repo)

    val state = vm.uiState.value
    assertFalse(state.loading)
    assertNull(state.error)
    assertTrue(state.sections.isNotEmpty())
  }

  @Test
  fun errorSetsErrorState() = runTest {
    val repo = FakeCategoriesRepository(error = "Network failure")
    val vm = CategoriesViewModel(repo)

    val state = vm.uiState.value
    assertFalse(state.loading)
    assertEquals("Network failure", state.error)
    assertTrue(state.sections.isEmpty())
  }

  @Test
  fun toggleSectionCollapsesAndExpands() = runTest {
    val repo = FakeCategoriesRepository(sampleCategories())
    val vm = CategoriesViewModel(repo)

    assertTrue(BudgetMode.MONTHLY in vm.uiState.value.expandedSections)

    vm.toggleSection(BudgetMode.MONTHLY)
    assertFalse(BudgetMode.MONTHLY in vm.uiState.value.expandedSections)

    vm.toggleSection(BudgetMode.MONTHLY)
    assertTrue(BudgetMode.MONTHLY in vm.uiState.value.expandedSections)
  }

  @Test
  fun categoriesGroupedByBudgetMode() = runTest {
    val sections = CategoriesViewModel.buildSections(sampleCategories())

    assertEquals(3, sections.size)
    assertEquals("Monthly", sections[0].label)
    assertEquals("Annual", sections[1].label)
    assertEquals("Unbudgeted", sections[2].label)

    assertEquals(BudgetMode.MONTHLY, sections[0].mode)
    assertEquals(BudgetMode.ANNUAL, sections[1].mode)
    assertNull(sections[2].mode)
  }

  @Test
  fun parentWithBudgetNestsUnbudgetedChildren() = runTest {
    val sections = CategoriesViewModel.buildSections(sampleCategories())

    val monthly = sections.first { it.mode == BudgetMode.MONTHLY }
    // "Food" (monthly), then its unbudgeted child "Dining Out", then "Groceries" (monthly)
    assertEquals(3, monthly.categories.size)
    assertEquals("Food", monthly.categories[0].name)
    assertFalse(monthly.categories[0].isChild)
    assertEquals("Dining Out", monthly.categories[1].name)
    assertTrue(monthly.categories[1].isChild)
    assertEquals("Food", monthly.categories[1].parentName)
    assertEquals("Groceries", monthly.categories[2].name)
    assertFalse(monthly.categories[2].isChild)
  }

  @Test
  fun childWithOwnBudgetModeAppearsInItsSection() = runTest {
    val categories =
        listOf(
            Category(id = "house", name = catName("House"), transactionCount = 0),
            Category(
                id = "mortgage",
                name = catName("Mortgage"),
                parentId = "house",
                budgetMode = BudgetMode.MONTHLY,
                budgetAmount = "4000",
                transactionCount = 12,
            ),
            Category(
                id = "cleaning",
                name = catName("Cleaning"),
                parentId = "house",
                transactionCount = 3,
            ),
        )
    val sections = CategoriesViewModel.buildSections(categories)

    // Mortgage has its own budget_mode=monthly, so it appears in Monthly
    val monthly = sections.first { it.mode == BudgetMode.MONTHLY }
    assertEquals(1, monthly.categories.size)
    assertEquals("Mortgage", monthly.categories[0].name)
    assertTrue(monthly.categories[0].isChild)
    assertEquals("House", monthly.categories[0].parentName)

    // House (unbudgeted root) appears in Unbudgeted with Cleaning nested under it
    val unbudgeted = sections.first { it.mode == null }
    assertEquals(2, unbudgeted.categories.size)
    assertEquals("House", unbudgeted.categories[0].name)
    assertEquals("Cleaning", unbudgeted.categories[1].name)
    assertTrue(unbudgeted.categories[1].isChild)
  }

  @Test
  fun allSectionsExpandedByDefault() = runTest {
    val repo = FakeCategoriesRepository(sampleCategories())
    val vm = CategoriesViewModel(repo)

    val state = vm.uiState.value
    for (section in state.sections) {
      assertTrue(
          section.mode in state.expandedSections,
          "Section ${section.label} should be expanded by default",
      )
    }
  }

  @Test
  fun sectionOrderIsCorrect() = runTest {
    val categories =
        listOf(
            Category(
                id = "1",
                name = catName("Rent"),
                budgetMode = BudgetMode.MONTHLY,
                budgetAmount = "1000",
                transactionCount = 5,
            ),
            Category(
                id = "2",
                name = catName("Insurance"),
                budgetMode = BudgetMode.ANNUAL,
                budgetAmount = "500",
                transactionCount = 2,
            ),
            Category(
                id = "3",
                name = catName("Renovation"),
                budgetMode = BudgetMode.PROJECT,
                budgetAmount = "5000",
                transactionCount = 10,
            ),
            Category(id = "4", name = catName("Random"), transactionCount = 1),
        )
    val sections = CategoriesViewModel.buildSections(categories)

    assertEquals(4, sections.size)
    assertEquals(BudgetMode.MONTHLY, sections[0].mode)
    assertEquals(BudgetMode.ANNUAL, sections[1].mode)
    assertEquals(BudgetMode.PROJECT, sections[2].mode)
    assertNull(sections[3].mode)
  }

  // -----------------------------------------------------------------------
  // Category name parsing / display tests
  // -----------------------------------------------------------------------

  @Test
  fun childWithLeafNameShowsParentName() = runTest {
    // Child stored as "Groceries" under parent "Food"
    val categories =
        listOf(
            Category(
                id = "food",
                name = catName("Food"),
                budgetMode = BudgetMode.MONTHLY,
                budgetAmount = "400",
                transactionCount = 10,
            ),
            Category(
                id = "groc",
                name = catName("Groceries"),
                parentId = "food",
                transactionCount = 5,
            ),
        )
    val sections = CategoriesViewModel.buildSections(categories)
    val monthly = sections.first { it.mode == BudgetMode.MONTHLY }
    val grocItem = monthly.categories.find { it.id == "groc" }!!
    assertEquals("Food", grocItem.parentName)
    assertEquals("Groceries", grocItem.name)
    assertTrue(grocItem.isChild)
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
