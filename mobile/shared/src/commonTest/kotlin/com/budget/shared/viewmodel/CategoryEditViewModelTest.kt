package com.budget.shared.viewmodel

import com.budget.shared.api.BudgetMode
import com.budget.shared.api.BudgetType
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
    val vm = CategoryEditViewModel(FakeCategoryEditRepository())

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
            name = catName("Groceries"),
            parentId = "food",
            budgetMode = BudgetMode.MONTHLY,
            budgetAmount = "500",
            transactionCount = 10,
        )
    val vm = CategoryEditViewModel(FakeCategoryEditRepository(), editingCategory = category)

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
            name = catName("Renovation"),
            budgetMode = BudgetMode.PROJECT,
            budgetAmount = "10000",
            projectStartDate = "2025-03-01",
            projectEndDate = "2025-06-30",
            transactionCount = 5,
        )
    val vm = CategoryEditViewModel(FakeCategoryEditRepository(), editingCategory = category)

    val state = vm.uiState.value
    assertEquals(BudgetMode.PROJECT, state.budgetMode)
    assertEquals("2025-03-01", state.projectStartDate)
    assertEquals("2025-06-30", state.projectEndDate)
  }

  @Test
  fun updateNameClearsError() = runTest {
    val vm = CategoryEditViewModel(FakeCategoryEditRepository())
    vm.save() // triggers "Name is required" error
    assertEquals("Name is required", vm.uiState.value.error)

    vm.updateName("Test")
    assertNull(vm.uiState.value.error)
    assertEquals("Test", vm.uiState.value.name)
  }

  @Test
  fun saveWithEmptyNameSetsError() = runTest {
    val vm = CategoryEditViewModel(FakeCategoryEditRepository())

    vm.save()

    val state = vm.uiState.value
    assertEquals("Name is required", state.error)
    assertFalse(state.saving)
    assertFalse(state.saved)
  }

  @Test
  fun saveWithBlankNameSetsError() = runTest {
    val vm = CategoryEditViewModel(FakeCategoryEditRepository())
    vm.updateName("   ")

    vm.save()

    assertEquals("Name is required", vm.uiState.value.error)
  }

  @Test
  fun saveNewCategoryCallsCreate() = runTest {
    val repo = FakeCategoryEditRepository()
    val vm = CategoryEditViewModel(repo)
    vm.updateName("Entertainment")
    vm.updateBudgetMode(BudgetMode.MONTHLY)
    vm.updateBudgetAmount("200")

    vm.save()

    assertTrue(vm.uiState.value.saved)
    assertEquals(1, repo.createdRequests.size)
    assertEquals(0, repo.updatedRequests.size)
    val req = repo.createdRequests[0]
    assertEquals("Entertainment", req.name.value)
    assertEquals("monthly", req.budgetMode)
    assertEquals("200", req.budgetAmount)
    assertNull(req.parentId)
  }

  @Test
  fun saveExistingCategoryCallsUpdate() = runTest {
    val category = Category(id = "cat-1", name = catName("Old Name"), transactionCount = 0)
    val repo = FakeCategoryEditRepository()
    val vm = CategoryEditViewModel(repo, editingCategory = category)
    vm.updateName("New Name")

    vm.save()

    assertTrue(vm.uiState.value.saved)
    assertEquals(0, repo.createdRequests.size)
    assertEquals(1, repo.updatedRequests.size)
    assertEquals("cat-1", repo.updatedIds[0])
    assertEquals("New Name", repo.updatedRequests[0].name.value)
  }

  @Test
  fun saveErrorSetsErrorState() = runTest {
    val repo = FakeCategoryEditRepository(saveError = "already exists")
    val vm = CategoryEditViewModel(repo)
    vm.updateName("Duplicate")

    vm.save()

    assertFalse(vm.uiState.value.saved)
    assertFalse(vm.uiState.value.saving)
    assertEquals("already exists", vm.uiState.value.error)
  }

  @Test
  fun updateFieldsMutateState() = runTest {
    val vm = CategoryEditViewModel(FakeCategoryEditRepository())

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
  fun saveWithColonInNameSetsError() = runTest {
    val vm = CategoryEditViewModel(FakeCategoryEditRepository())
    vm.updateName("Food:Groceries")

    vm.save()

    val state = vm.uiState.value
    assertEquals("name contains a colon \u2014 use parent_id for hierarchy", state.error)
    assertFalse(state.saved)
  }

  @Test
  fun saveWithControlCharInNameSetsError() = runTest {
    val vm = CategoryEditViewModel(FakeCategoryEditRepository())
    vm.updateName("Bad\u0000Name")

    vm.save()

    val state = vm.uiState.value
    assertEquals("name contains control characters", state.error)
    assertFalse(state.saved)
  }

  @Test
  fun buildRequestOmitsBudgetAmountWhenNoMode() {
    val state = CategoryEditUiState(name = "Test", budgetAmount = "500")
    val request = CategoryEditViewModel.buildRequest(state, catName("Test"))

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
    val request = CategoryEditViewModel.buildRequest(state, catName("Test"))

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
    val request = CategoryEditViewModel.buildRequest(state, catName("Reno"))

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
    val request = CategoryEditViewModel.buildRequest(state, catName("Test"))

    assertNull(request.projectStartDate)
    assertNull(request.projectEndDate)
  }

  @Test
  fun loadsAvailableParentsExcludingEditedCategory() = runTest {
    val categories =
        listOf(
            Category(id = "a", name = catName("Alpha"), transactionCount = 0),
            Category(id = "b", name = catName("Beta"), transactionCount = 0),
            Category(id = "c", name = catName("Child"), parentId = "a", transactionCount = 0),
        )
    val editing = Category(id = "a", name = catName("Alpha"), transactionCount = 0)
    val vm =
        CategoryEditViewModel(
            FakeCategoryEditRepository(),
            editingCategory = editing,
            allCategories = categories,
        )

    val parents = vm.uiState.value.availableParents
    // "a" excluded (self), "c" excluded (has parent), only "b" remains
    assertEquals(1, parents.size)
    assertEquals("b", parents[0].id)
    assertEquals("Beta", parents[0].name)
  }

  @Test
  fun editingSubcategoryIncludesCurrentParentInAvailableParents() = runTest {
    val categories =
        listOf(
            Category(id = "a", name = catName("Alpha"), transactionCount = 0),
            Category(id = "b", name = catName("Beta"), transactionCount = 0),
            Category(id = "c", name = catName("Child"), parentId = "a", transactionCount = 0),
        )
    val editing = Category(id = "c", name = catName("Child"), parentId = "a", transactionCount = 0)
    val vm =
        CategoryEditViewModel(
            FakeCategoryEditRepository(),
            editingCategory = editing,
            allCategories = categories,
        )

    val state = vm.uiState.value
    assertEquals("a", state.parentId)
    val parents = state.availableParents
    // "a" and "b" are root categories, "c" excluded (self)
    assertEquals(2, parents.size)
    val parentNames = parents.map { it.name }.toSet()
    assertTrue("Alpha" in parentNames)
    assertTrue("Beta" in parentNames)
    // Current parent resolves to a name (not "None")
    val selectedName = parents.find { it.id == state.parentId }?.name
    assertEquals("Alpha", selectedName)
  }

  @Test
  fun editingNestedSubcategoryIncludesNonRootParent() = runTest {
    val categories =
        listOf(
            Category(id = "a", name = catName("Root"), transactionCount = 0),
            Category(id = "b", name = catName("Mid"), parentId = "a", transactionCount = 0),
            Category(id = "c", name = catName("Leaf"), parentId = "b", transactionCount = 0),
        )
    val editing = Category(id = "c", name = catName("Leaf"), parentId = "b", transactionCount = 0)
    val vm =
        CategoryEditViewModel(
            FakeCategoryEditRepository(),
            editingCategory = editing,
            allCategories = categories,
        )

    val state = vm.uiState.value
    assertEquals("b", state.parentId)
    // "b" has parentId != null but is included because it's the current parent
    val selectedName = state.availableParents.find { it.id == state.parentId }?.name
    assertEquals("Mid", selectedName)
  }

  @Test
  fun newCategoryShowsAllRootCategoriesAsParents() = runTest {
    val categories =
        listOf(
            Category(id = "a", name = catName("Alpha"), transactionCount = 0),
            Category(id = "b", name = catName("Beta"), transactionCount = 0),
            Category(id = "c", name = catName("Child"), parentId = "a", transactionCount = 0),
        )
    val vm = CategoryEditViewModel(FakeCategoryEditRepository(), allCategories = categories)

    val parents = vm.uiState.value.availableParents
    // Only root categories shown; "c" excluded (has parent)
    assertEquals(2, parents.size)
    assertEquals("Alpha", parents[0].name)
    assertEquals("Beta", parents[1].name)
  }

  @Test
  fun editingSubcategoryDoesNotDuplicateCurrentParent() = runTest {
    // Parent "a" is already a root category (parentId == null), so it qualifies
    // both as a root and as the current parent — should not appear twice
    val categories =
        listOf(
            Category(id = "a", name = catName("Food"), transactionCount = 0),
            Category(id = "b", name = catName("Groceries"), parentId = "a", transactionCount = 0),
        )
    val editing =
        Category(id = "b", name = catName("Groceries"), parentId = "a", transactionCount = 0)
    val vm =
        CategoryEditViewModel(
            FakeCategoryEditRepository(),
            editingCategory = editing,
            allCategories = categories,
        )

    val parents = vm.uiState.value.availableParents
    assertEquals(1, parents.size)
    assertEquals("Food", parents[0].name)
  }

  @Test
  fun editingRootCategoryExcludesSelfFromParents() = runTest {
    val categories =
        listOf(
            Category(id = "a", name = catName("Alpha"), transactionCount = 0),
            Category(id = "b", name = catName("Beta"), transactionCount = 0),
        )
    val editing = Category(id = "a", name = catName("Alpha"), transactionCount = 0)
    val vm =
        CategoryEditViewModel(
            FakeCategoryEditRepository(),
            editingCategory = editing,
            allCategories = categories,
        )

    val state = vm.uiState.value
    assertNull(state.parentId)
    val parents = state.availableParents
    assertEquals(1, parents.size)
    assertEquals("Beta", parents[0].name)
  }

  @Test
  fun editingSubcategoryWithDeletedParentShowsNone() = runTest {
    // Parent "x" no longer exists in the categories list
    val categories =
        listOf(
            Category(id = "a", name = catName("Alpha"), transactionCount = 0),
        )
    val editing = Category(id = "b", name = catName("Orphan"), parentId = "x", transactionCount = 0)
    val vm =
        CategoryEditViewModel(
            FakeCategoryEditRepository(),
            editingCategory = editing,
            allCategories = categories,
        )

    val state = vm.uiState.value
    assertEquals("x", state.parentId)
    // Parent "x" doesn't exist, so it can't be resolved
    val selectedName = state.availableParents.find { it.id == state.parentId }?.name
    assertNull(selectedName)
  }

  @Test
  fun modeToStringConversions() {
    assertEquals("monthly", CategoryEditViewModel.modeToString(BudgetMode.MONTHLY))
    assertEquals("annual", CategoryEditViewModel.modeToString(BudgetMode.ANNUAL))
    assertEquals("project", CategoryEditViewModel.modeToString(BudgetMode.PROJECT))
    assertEquals("salary", CategoryEditViewModel.modeToString(BudgetMode.SALARY))
  }

  @Test
  fun typeToStringConversions() {
    assertEquals("fixed", CategoryEditViewModel.typeToString(BudgetType.FIXED))
    assertEquals("variable", CategoryEditViewModel.typeToString(BudgetType.VARIABLE))
  }

  @Test
  fun updateBudgetTypeMutatesState() = runTest {
    val vm = CategoryEditViewModel(FakeCategoryEditRepository())
    assertEquals(BudgetType.VARIABLE, vm.uiState.value.budgetType)

    vm.updateBudgetType(BudgetType.FIXED)
    assertEquals(BudgetType.FIXED, vm.uiState.value.budgetType)
  }

  @Test
  fun editingFixedCategoryPopulatesBudgetType() = runTest {
    val category =
        Category(
            id = "cat-1",
            name = catName("Mortgage"),
            budgetMode = BudgetMode.MONTHLY,
            budgetType = BudgetType.FIXED,
            budgetAmount = "2000",
            transactionCount = 5,
        )
    val vm = CategoryEditViewModel(FakeCategoryEditRepository(), editingCategory = category)

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
    val request = CategoryEditViewModel.buildRequest(state, catName("Mortgage"))

    assertEquals("fixed", request.budgetType)
  }

  @Test
  fun buildRequestOmitsBudgetTypeWhenNoMode() {
    val state =
        CategoryEditUiState(
            name = "Test",
            budgetType = BudgetType.FIXED,
        )
    val request = CategoryEditViewModel.buildRequest(state, catName("Test"))

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
    val request = CategoryEditViewModel.buildRequest(state, catName("Groceries"))

    assertEquals("variable", request.budgetType)
  }

  @Test
  fun nameTrimmedBeforeSave() = runTest {
    val repo = FakeCategoryEditRepository()
    val vm = CategoryEditViewModel(repo)
    vm.updateName("  Groceries  ")

    vm.save()

    assertTrue(vm.uiState.value.saved)
    assertEquals("Groceries", repo.createdRequests[0].name.value)
  }
}

// -- Helpers ----------------------------------------------------------------

private fun catName(name: String): CategoryName = CategoryName.of(name).getOrThrow()

// -- Test doubles -----------------------------------------------------------

private class FakeCategoryEditRepository(
    private val saveError: String? = null,
) : BudgetRepository {

  val createdRequests = mutableListOf<CategoryRequest>()
  val updatedRequests = mutableListOf<CategoryRequest>()
  val updatedIds = mutableListOf<String>()

  private val _invalidationEvents = MutableSharedFlow<InvalidationEvent>()
  override val invalidationEvents: SharedFlow<InvalidationEvent> =
      _invalidationEvents.asSharedFlow()

  override suspend fun getDashboardData(monthId: String?): DashboardData =
      throw NotImplementedError()

  override suspend fun getTransactions(limit: Int, offset: Int, categoryId: String?) =
      TransactionPage(emptyList(), 0, limit, offset)

  override suspend fun getTransaction(id: String): Transaction = throw NotImplementedError()

  override suspend fun getCategories(): List<Category> = emptyList()

  override suspend fun categorizeTransaction(transactionId: String, categoryId: String) = true

  override suspend fun uncategorizeTransaction(transactionId: String) = true

  override suspend fun createCategory(request: CategoryRequest): Category {
    if (saveError != null) throw RuntimeException(saveError)
    createdRequests.add(request)
    return Category(id = "new-id", name = request.name, transactionCount = 0)
  }

  override suspend fun updateCategory(id: String, request: CategoryRequest): Category {
    if (saveError != null) throw RuntimeException(saveError)
    updatedIds.add(id)
    updatedRequests.add(request)
    return Category(id = id, name = request.name, transactionCount = 0)
  }
}
