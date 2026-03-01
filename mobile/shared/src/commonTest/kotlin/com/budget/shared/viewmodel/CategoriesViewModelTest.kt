package com.budget.shared.viewmodel

import com.budget.shared.api.BudgetMode
import com.budget.shared.api.Category
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
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
    val fetcher = FakeCategoriesFetcher(sampleCategories())
    val vm = CategoriesViewModel("https://example.com", "key", fetcher)

    val state = vm.uiState.value
    assertFalse(state.loading)
    assertNull(state.error)
    assertTrue(state.sections.isNotEmpty())
  }

  @Test
  fun errorSetsErrorState() = runTest {
    val fetcher = FakeCategoriesFetcher(error = "Network failure")
    val vm = CategoriesViewModel("https://example.com", "key", fetcher)

    val state = vm.uiState.value
    assertFalse(state.loading)
    assertEquals("Network failure", state.error)
    assertTrue(state.sections.isEmpty())
  }

  @Test
  fun toggleSectionCollapsesAndExpands() = runTest {
    val fetcher = FakeCategoriesFetcher(sampleCategories())
    val vm = CategoriesViewModel("https://example.com", "key", fetcher)

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
            Category(id = "house", name = "House", transactionCount = 0),
            Category(
                id = "mortgage",
                name = "Mortgage",
                parentId = "house",
                budgetMode = BudgetMode.MONTHLY,
                budgetAmount = "4000",
                transactionCount = 12,
            ),
            Category(
                id = "cleaning",
                name = "Cleaning",
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
    val fetcher = FakeCategoriesFetcher(sampleCategories())
    val vm = CategoriesViewModel("https://example.com", "key", fetcher)

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
                name = "Rent",
                budgetMode = BudgetMode.MONTHLY,
                budgetAmount = "1000",
                transactionCount = 5,
            ),
            Category(
                id = "2",
                name = "Insurance",
                budgetMode = BudgetMode.ANNUAL,
                budgetAmount = "500",
                transactionCount = 2,
            ),
            Category(
                id = "3",
                name = "Renovation",
                budgetMode = BudgetMode.PROJECT,
                budgetAmount = "5000",
                transactionCount = 10,
            ),
            Category(id = "4", name = "Random", transactionCount = 1),
        )
    val sections = CategoriesViewModel.buildSections(categories)

    assertEquals(4, sections.size)
    assertEquals(BudgetMode.MONTHLY, sections[0].mode)
    assertEquals(BudgetMode.ANNUAL, sections[1].mode)
    assertEquals(BudgetMode.PROJECT, sections[2].mode)
    assertNull(sections[3].mode)
  }
}

// -- Test doubles -----------------------------------------------------------

private class FakeCategoriesFetcher(
    private val categories: List<Category> = emptyList(),
    private val error: String? = null,
) : CategoriesFetcher {
  override suspend fun fetchCategories(serverUrl: String, apiKey: String): List<Category> {
    if (error != null) throw RuntimeException(error)
    return categories
  }
}

private fun sampleCategories(): List<Category> =
    listOf(
        Category(
            id = "food",
            name = "Food",
            budgetMode = BudgetMode.MONTHLY,
            budgetAmount = "400",
            transactionCount = 20,
        ),
        Category(
            id = "dining",
            name = "Dining Out",
            parentId = "food",
            // budgetMode is null — inherited from parent
            budgetAmount = "100",
            transactionCount = 8,
        ),
        Category(
            id = "groceries",
            name = "Groceries",
            budgetMode = BudgetMode.MONTHLY,
            budgetAmount = "300",
            transactionCount = 15,
        ),
        Category(
            id = "insurance",
            name = "Insurance",
            budgetMode = BudgetMode.ANNUAL,
            budgetAmount = "1200",
            transactionCount = 4,
        ),
        Category(id = "misc", name = "Miscellaneous", transactionCount = 3),
    )
