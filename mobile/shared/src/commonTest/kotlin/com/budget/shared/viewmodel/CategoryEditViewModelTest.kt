package com.budget.shared.viewmodel

import com.budget.shared.api.BudgetMode
import com.budget.shared.api.BudgetType
import com.budget.shared.api.Category
import com.budget.shared.api.CategoryRequest
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
class CategoryEditViewModelTest {

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
  fun newCategoryStartsWithEmptyState() = runTest {
    val vm = CategoryEditViewModel("https://example.com", "key", saver = FakeCategorySaver())

    val state = vm.uiState.value
    assertFalse(state.isEditing)
    assertEquals("", state.name)
    assertNull(state.parentId)
    assertNull(state.budgetMode)
    assertEquals(BudgetType.VARIABLE, state.budgetType)
    assertEquals("", state.budgetAmount)
    assertFalse(state.saving)
    assertFalse(state.saved)
    assertNull(state.error)
  }

  @Test
  fun editingCategoryPopulatesState() = runTest {
    val category =
        Category(
            id = "cat-1",
            name = "Groceries",
            parentId = "food",
            budgetMode = BudgetMode.MONTHLY,
            budgetAmount = "500",
            transactionCount = 10,
        )
    val vm =
        CategoryEditViewModel(
            "https://example.com",
            "key",
            editingCategory = category,
            saver = FakeCategorySaver(),
        )

    val state = vm.uiState.value
    assertTrue(state.isEditing)
    assertEquals("Groceries", state.name)
    assertEquals("food", state.parentId)
    assertEquals(BudgetMode.MONTHLY, state.budgetMode)
    assertEquals("500", state.budgetAmount)
  }

  @Test
  fun editingProjectCategoryPopulatesDates() = runTest {
    val category =
        Category(
            id = "proj-1",
            name = "Renovation",
            budgetMode = BudgetMode.PROJECT,
            budgetAmount = "10000",
            projectStartDate = "2025-03-01",
            projectEndDate = "2025-06-30",
            transactionCount = 5,
        )
    val vm =
        CategoryEditViewModel(
            "https://example.com",
            "key",
            editingCategory = category,
            saver = FakeCategorySaver(),
        )

    val state = vm.uiState.value
    assertEquals(BudgetMode.PROJECT, state.budgetMode)
    assertEquals("2025-03-01", state.projectStartDate)
    assertEquals("2025-06-30", state.projectEndDate)
  }

  @Test
  fun updateNameClearsError() = runTest {
    val vm = CategoryEditViewModel("https://example.com", "key", saver = FakeCategorySaver())
    vm.save() // triggers "Name is required" error
    assertEquals("Name is required", vm.uiState.value.error)

    vm.updateName("Test")
    assertNull(vm.uiState.value.error)
    assertEquals("Test", vm.uiState.value.name)
  }

  @Test
  fun saveWithEmptyNameSetsError() = runTest {
    val vm = CategoryEditViewModel("https://example.com", "key", saver = FakeCategorySaver())

    vm.save()

    val state = vm.uiState.value
    assertEquals("Name is required", state.error)
    assertFalse(state.saving)
    assertFalse(state.saved)
  }

  @Test
  fun saveWithBlankNameSetsError() = runTest {
    val vm = CategoryEditViewModel("https://example.com", "key", saver = FakeCategorySaver())
    vm.updateName("   ")

    vm.save()

    assertEquals("Name is required", vm.uiState.value.error)
  }

  @Test
  fun saveNewCategoryCallsCreate() = runTest {
    val saver = FakeCategorySaver()
    val vm = CategoryEditViewModel("https://example.com", "key", saver = saver)
    vm.updateName("Entertainment")
    vm.updateBudgetMode(BudgetMode.MONTHLY)
    vm.updateBudgetAmount("200")

    vm.save()

    assertTrue(vm.uiState.value.saved)
    assertEquals(1, saver.createdRequests.size)
    assertEquals(0, saver.updatedRequests.size)
    val req = saver.createdRequests[0]
    assertEquals("Entertainment", req.name)
    assertEquals("monthly", req.budgetMode)
    assertEquals("200", req.budgetAmount)
    assertNull(req.parentId)
  }

  @Test
  fun saveExistingCategoryCallsUpdate() = runTest {
    val category = Category(id = "cat-1", name = "Old Name", transactionCount = 0)
    val saver = FakeCategorySaver()
    val vm =
        CategoryEditViewModel(
            "https://example.com",
            "key",
            editingCategory = category,
            saver = saver,
        )
    vm.updateName("New Name")

    vm.save()

    assertTrue(vm.uiState.value.saved)
    assertEquals(0, saver.createdRequests.size)
    assertEquals(1, saver.updatedRequests.size)
    assertEquals("cat-1", saver.updatedIds[0])
    assertEquals("New Name", saver.updatedRequests[0].name)
  }

  @Test
  fun saveErrorSetsErrorState() = runTest {
    val saver = FakeCategorySaver(error = "already exists")
    val vm = CategoryEditViewModel("https://example.com", "key", saver = saver)
    vm.updateName("Duplicate")

    vm.save()

    assertFalse(vm.uiState.value.saved)
    assertFalse(vm.uiState.value.saving)
    assertEquals("already exists", vm.uiState.value.error)
  }

  @Test
  fun updateFieldsMutateState() = runTest {
    val vm = CategoryEditViewModel("https://example.com", "key", saver = FakeCategorySaver())

    vm.updateParentId("parent-1")
    assertEquals("parent-1", vm.uiState.value.parentId)

    vm.updateBudgetMode(BudgetMode.ANNUAL)
    assertEquals(BudgetMode.ANNUAL, vm.uiState.value.budgetMode)

    vm.updateBudgetAmount("1200")
    assertEquals("1200", vm.uiState.value.budgetAmount)

    vm.updateProjectStartDate("2025-01-01")
    assertEquals("2025-01-01", vm.uiState.value.projectStartDate)

    vm.updateProjectEndDate("2025-12-31")
    assertEquals("2025-12-31", vm.uiState.value.projectEndDate)
  }

  @Test
  fun buildRequestOmitsBudgetAmountWhenNoMode() {
    val state = CategoryEditUiState(name = "Test", budgetAmount = "500")
    val request = CategoryEditViewModel.buildRequest(state, "Test")

    assertNull(request.budgetMode)
    assertNull(request.budgetAmount)
  }

  @Test
  fun buildRequestIncludesBudgetAmountWithMode() {
    val state =
        CategoryEditUiState(
            name = "Test",
            budgetMode = BudgetMode.MONTHLY,
            budgetAmount = "500",
        )
    val request = CategoryEditViewModel.buildRequest(state, "Test")

    assertEquals("monthly", request.budgetMode)
    assertEquals("500", request.budgetAmount)
  }

  @Test
  fun buildRequestIncludesProjectDates() {
    val state =
        CategoryEditUiState(
            name = "Reno",
            budgetMode = BudgetMode.PROJECT,
            budgetAmount = "10000",
            projectStartDate = "2025-03-01",
            projectEndDate = "2025-06-30",
        )
    val request = CategoryEditViewModel.buildRequest(state, "Reno")

    assertEquals("project", request.budgetMode)
    assertEquals("2025-03-01", request.projectStartDate)
    assertEquals("2025-06-30", request.projectEndDate)
  }

  @Test
  fun buildRequestOmitsProjectDatesForNonProjectMode() {
    val state =
        CategoryEditUiState(
            name = "Test",
            budgetMode = BudgetMode.MONTHLY,
            budgetAmount = "300",
            projectStartDate = "2025-03-01",
            projectEndDate = "2025-06-30",
        )
    val request = CategoryEditViewModel.buildRequest(state, "Test")

    assertNull(request.projectStartDate)
    assertNull(request.projectEndDate)
  }

  @Test
  fun loadsAvailableParentsExcludingEditedCategory() = runTest {
    val categories =
        listOf(
            Category(id = "a", name = "Alpha", transactionCount = 0),
            Category(id = "b", name = "Beta", transactionCount = 0),
            Category(id = "c", name = "Child", parentId = "a", transactionCount = 0),
        )
    val editing = Category(id = "a", name = "Alpha", transactionCount = 0)
    val saver = FakeCategorySaver(categories = categories)
    val vm =
        CategoryEditViewModel(
            "https://example.com",
            "key",
            editingCategory = editing,
            saver = saver,
        )

    val parents = vm.uiState.value.availableParents
    // "a" excluded (self), "c" excluded (has parent), only "b" remains
    assertEquals(1, parents.size)
    assertEquals("b", parents[0].id)
    assertEquals("Beta", parents[0].name)
  }

  @Test
  fun modeToStringConversions() {
    assertEquals("monthly", CategoryEditViewModel.modeToString(BudgetMode.MONTHLY))
    assertEquals("annual", CategoryEditViewModel.modeToString(BudgetMode.ANNUAL))
    assertEquals("project", CategoryEditViewModel.modeToString(BudgetMode.PROJECT))
  }

  @Test
  fun typeToStringConversions() {
    assertEquals("fixed", CategoryEditViewModel.typeToString(BudgetType.FIXED))
    assertEquals("variable", CategoryEditViewModel.typeToString(BudgetType.VARIABLE))
  }

  @Test
  fun updateBudgetTypeMutatesState() = runTest {
    val vm = CategoryEditViewModel("https://example.com", "key", saver = FakeCategorySaver())
    assertEquals(BudgetType.VARIABLE, vm.uiState.value.budgetType)

    vm.updateBudgetType(BudgetType.FIXED)
    assertEquals(BudgetType.FIXED, vm.uiState.value.budgetType)
  }

  @Test
  fun editingFixedCategoryPopulatesBudgetType() = runTest {
    val category =
        Category(
            id = "cat-1",
            name = "Mortgage",
            budgetMode = BudgetMode.MONTHLY,
            budgetType = BudgetType.FIXED,
            budgetAmount = "2000",
            transactionCount = 5,
        )
    val vm =
        CategoryEditViewModel(
            "https://example.com",
            "key",
            editingCategory = category,
            saver = FakeCategorySaver(),
        )

    assertEquals(BudgetType.FIXED, vm.uiState.value.budgetType)
  }

  @Test
  fun buildRequestIncludesBudgetType() {
    val state =
        CategoryEditUiState(
            name = "Mortgage",
            budgetMode = BudgetMode.MONTHLY,
            budgetType = BudgetType.FIXED,
            budgetAmount = "2000",
        )
    val request = CategoryEditViewModel.buildRequest(state, "Mortgage")

    assertEquals("fixed", request.budgetType)
  }

  @Test
  fun buildRequestOmitsBudgetTypeWhenNoMode() {
    val state =
        CategoryEditUiState(
            name = "Test",
            budgetType = BudgetType.FIXED,
        )
    val request = CategoryEditViewModel.buildRequest(state, "Test")

    assertNull(request.budgetType)
  }

  @Test
  fun buildRequestDefaultsToVariableBudgetType() {
    val state =
        CategoryEditUiState(
            name = "Groceries",
            budgetMode = BudgetMode.MONTHLY,
            budgetAmount = "500",
        )
    val request = CategoryEditViewModel.buildRequest(state, "Groceries")

    assertEquals("variable", request.budgetType)
  }

  @Test
  fun nameTrimmedBeforeSave() = runTest {
    val saver = FakeCategorySaver()
    val vm = CategoryEditViewModel("https://example.com", "key", saver = saver)
    vm.updateName("  Groceries  ")

    vm.save()

    assertTrue(vm.uiState.value.saved)
    assertEquals("Groceries", saver.createdRequests[0].name)
  }
}

// -- Test doubles -----------------------------------------------------------

private class FakeCategorySaver(
    private val categories: List<Category> = emptyList(),
    private val error: String? = null,
) : CategorySaver {

  val createdRequests = mutableListOf<CategoryRequest>()
  val updatedRequests = mutableListOf<CategoryRequest>()
  val updatedIds = mutableListOf<String>()

  override suspend fun fetchCategories(serverUrl: String, apiKey: String): List<Category> {
    return categories
  }

  override suspend fun createCategory(
      serverUrl: String,
      apiKey: String,
      request: CategoryRequest,
  ): Category {
    if (error != null) throw RuntimeException(error)
    createdRequests.add(request)
    return Category(id = "new-id", name = request.name, transactionCount = 0)
  }

  override suspend fun updateCategory(
      serverUrl: String,
      apiKey: String,
      id: String,
      request: CategoryRequest,
  ): Category {
    if (error != null) throw RuntimeException(error)
    updatedIds.add(id)
    updatedRequests.add(request)
    return Category(id = id, name = request.name, transactionCount = 0)
  }
}
