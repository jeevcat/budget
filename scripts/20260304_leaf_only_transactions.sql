-- Restructure categories so every transaction sits on a leaf category.
--
-- Changes:
--   1. Rename top-level "Insurance" → "Health"; rename its child "Health" → "Insurance"
--   2. Create new leaf categories: Fitness, Kindergeld, Investments, Other, Clothing, Household, Donations
--   3. Reassign 73 transactions from parent categories to appropriate leaves
--   4. Uncategorize 2 AMEX credit-card-bill transactions that were misclassified as Insurance
--   5. Add Kienle Transport → House > Renovation auto-categorization rule

-- ============================================================
-- 1. Rename Insurance ↔ Health
-- ============================================================

-- Rename child first to avoid temporary name collision
UPDATE categories SET name = 'Insurance' WHERE id = '0b887dca-63dd-405e-a86b-6fdfc2885412';
UPDATE categories SET name = 'Health'    WHERE id = '9fb5006e-305f-4a78-8e1d-53af00d73cd7';

-- ============================================================
-- 2. Create new leaf categories
-- ============================================================

-- Health > Fitness
INSERT INTO categories (id, name, parent_id)
VALUES ('a0000001-0000-0000-0000-000000000001', 'Fitness', '9fb5006e-305f-4a78-8e1d-53af00d73cd7');

-- Income > Kindergeld
INSERT INTO categories (id, name, parent_id)
VALUES ('a0000001-0000-0000-0000-000000000002', 'Kindergeld', '774676b9-5bf7-4742-aba1-97e1d01a6b96');

-- Income > Investments
INSERT INTO categories (id, name, parent_id)
VALUES ('a0000001-0000-0000-0000-000000000003', 'Investments', '774676b9-5bf7-4742-aba1-97e1d01a6b96');

-- Income > Other
INSERT INTO categories (id, name, parent_id)
VALUES ('a0000001-0000-0000-0000-000000000004', 'Other', '774676b9-5bf7-4742-aba1-97e1d01a6b96');

-- Shopping > Clothing
INSERT INTO categories (id, name, parent_id)
VALUES ('a0000001-0000-0000-0000-000000000005', 'Clothing', '525282ed-fc6f-4d7f-a17e-512131b0689d');

-- Shopping > Household
INSERT INTO categories (id, name, parent_id)
VALUES ('a0000001-0000-0000-0000-000000000006', 'Household', '525282ed-fc6f-4d7f-a17e-512131b0689d');

-- Family > Donations
INSERT INTO categories (id, name, parent_id)
VALUES ('a0000001-0000-0000-0000-000000000007', 'Donations', '29468f70-4f43-42c5-b54d-9335a317a589');

-- ============================================================
-- 3. Reassign transactions from parent categories to leaves
-- ============================================================

-- Entertainment (parent: a1f29b85) → children
-- Fitness Express → Health > Fitness
UPDATE transactions SET category_id = 'a0000001-0000-0000-0000-000000000001'
WHERE id = '5b4491dc-e218-42d6-b763-252f0a9c4c2d';

-- Remaining 7 Entertainment txns → Entertainment > Holidays (25fd2c57)
UPDATE transactions SET category_id = '25fd2c57-2ac4-4828-8aef-3d76e42ff337'
WHERE id IN (
  'e65f72fd-4804-4f05-8cb7-83ddeffd79bf',  -- CTS Eventim
  '6572d322-fd82-4b9b-a73b-a6821fdaa3ae',  -- Massage Gutscheine
  '9d884b45-71c0-4958-8076-fae89673036d',  -- Kitzbühel Rote Teufel
  'a6cb7ac8-fce1-4a19-b6de-17d23b9fd5bb',  -- Landesmuseum
  '846b0c51-6ef6-447c-bc0e-afc6ece1370b',  -- Soccer Arena
  '4a64c6b7-4c45-4050-b262-c32aba77ef4f',  -- Soccer Arena
  '5612f79c-3bd9-422d-91d5-bab3c9965285'   -- Sport Schädle
);

-- Family (parent: 29468f70) → Family > Kids Activities (0d9f9e9e)
UPDATE transactions SET category_id = '0d9f9e9e-b1c9-449d-8040-9add797b684c'
WHERE id = 'e9600f62-1d59-4241-ba69-a7e0727a4f2a';

-- Fees (parent: aeec4088) → Fees > Bank Fees (1185e77c)
UPDATE transactions SET category_id = '1185e77c-3e19-42c2-b335-cf62a4fb0254'
WHERE id IN (
  '48c7e809-daea-4db4-9671-d555f320c166',  -- Verlustverrechnung
  '5e1941d5-119c-45e7-bdf1-d3217adf3a50',  -- PayPal fee
  '9f6a428a-2c92-4979-9f33-f01537cdb2a2'   -- Landratsamt
);

-- Food (parent: a5d32d8e) → Food > Restaurants (f5b416b6)
UPDATE transactions SET category_id = 'f5b416b6-1c32-427d-b6f3-ec551bd6f872'
WHERE id = '400654bf-e2af-4157-8af1-15ab0035138a';

-- Income (parent: 774676b9) → new leaf children
-- Familienkasse → Income > Kindergeld
UPDATE transactions SET category_id = 'a0000001-0000-0000-0000-000000000002'
WHERE id IN (
  'cd3d95b3-dc7f-4dfb-9ee0-9ce869b7c026',
  'ed7d267b-11f5-40b1-9adb-c528f6fde747',
  '3fb31504-b703-4b44-8edb-8c523400750b'
);

-- Jeeves Smart Invest → Income > Investments
UPDATE transactions SET category_id = 'a0000001-0000-0000-0000-000000000003'
WHERE id IN (
  '591bbdcc-7c69-44af-9f4e-6f288ae02f4f',
  '86ab158c-94d1-4a90-b0b6-4724c8406724',
  'd4690794-0591-4f31-af94-a567e6d1e655',
  '64963267-f7d0-401c-979b-90bea4a776dd',
  '84f35c4c-047d-436a-b46d-fd7935d56817',
  '9769cffd-07a5-48b3-af93-44d0c17ebe9f'
);

-- Roswitha Ziegler "sprit" → Income > Other
UPDATE transactions SET category_id = 'a0000001-0000-0000-0000-000000000004'
WHERE id = 'e3627542-9c3e-4b80-950a-bdc3f29b779f';

-- Insurance (parent: 9fb5006e, now renamed to Health)
-- ADAC, AGILA x3, HUK x3 → Health > Insurance (0b887dca, renamed from Health)
UPDATE transactions SET category_id = '0b887dca-63dd-405e-a86b-6fdfc2885412'
WHERE id IN (
  'fb588fbb-3dab-4d4e-af9a-a09407bb4fcd',  -- ADAC
  '3b9b96ea-6402-4cda-a38c-e4bef8f95f7c',  -- AGILA
  '99733b9c-4c63-410f-bc9b-e4e5ac6bcba3',  -- AGILA
  '11f77ca2-d156-42ca-bae3-8f496ce9b9f8',  -- AGILA
  '8d203cc0-9899-469f-8f6b-28ee0d44c414',  -- HUK
  'c54fe5f8-846d-4c9e-ae67-4c14875943c3',  -- HUK
  '72750de8-9706-4f7e-a238-21c3718dae7f'   -- HUK return
);

-- AMEX credit card payments → uncategorize
UPDATE transactions SET category_id = NULL, category_method = NULL
WHERE id IN (
  '131ce9a6-4730-4480-9b9a-7c9dd74ff3e0',
  '6c5214f7-030d-4bb2-a20c-4295ad79f248'
);

-- Shopping (parent: 525282ed) → various leaves
-- → Shopping > Clothing
UPDATE transactions SET category_id = 'a0000001-0000-0000-0000-000000000005'
WHERE id IN (
  'bc995128-7840-4680-b007-e405bf3f8a25',  -- Zalando
  '0daa48d7-68ca-40b1-8151-9b0ea0f04d58',  -- SportScheck
  'd7fc10c5-acf2-48ee-a0db-ce5ba3a66af5',  -- Uniqlo
  'c274ca50-854d-43e2-b0af-299e626895bb',  -- Dilling
  'ca5ae2b0-7a64-41d3-9c55-5404eaae43d3',  -- KIK
  'b6a1db84-92b5-41ea-bc65-9b74a05ddee1',  -- KIK
  '9e7c9397-0d51-417f-8dd9-aee15025cd63',  -- KIK
  'a3ca0b29-3cb5-4d46-94ec-6953f69c0472',  -- Decathlon refund
  '386e66c6-47a2-4640-8c58-f7584180cf47',  -- Little Ban Ban
  'ce01e904-e735-4f6a-8045-2e09b54baba6',  -- Sport Müller
  '7056ae34-f5d4-4cb3-8ece-a0b95f5ca523',  -- S Um und Auf
  'b8f7c2bf-0a6a-4f72-9681-75ea7e5ee3a7',  -- PayPal generic €223.49
  'a5007dcd-83b4-4f0e-9056-156ff66c2bb7'   -- PayPal unknown €43
);

-- → Shopping > Household
UPDATE transactions SET category_id = 'a0000001-0000-0000-0000-000000000006'
WHERE id IN (
  '2ab3880a-9589-4225-900c-197b6f5aa211',  -- Wundermix
  'dfd3e035-88cf-4757-b3ec-6c9d6219dbcb',  -- Frölich & Kaufmann refund
  '6c125f93-3b26-4569-842d-2fc8cef62b77',  -- DM drugstore
  '57bc0ccd-fc2d-4a7e-9fe9-90cb994a37a1',  -- Koehler
  '93a1b71d-7e67-4a96-824c-6e87b5c6486c',  -- Koehler
  '6f06aa87-3fbf-42eb-b674-d2d58300ecbd',  -- Yavuz Abay
  '29770e8e-6038-41da-9645-360f633be73e',  -- Yavuz Abay
  'e4337db9-c2be-4f16-ba9e-f48bbe915310',  -- Yavuz Abay
  'b27300f4-a91f-4ab5-bd2a-38c6e1b162ef',  -- Yavuz Abay
  '110dda55-880a-425d-8bd2-dbbedee458e9',  -- PH Mercaden
  '3b9b2c35-b20b-458d-bba8-05cfa8753fbc',  -- Jetzt kommt Kurth
  '02c36022-d1b0-42f8-97ac-51d2afb8205c',  -- LS Gesellschafter
  '13cf2b27-1b2a-48e7-9d3d-42ab8872cf4b'   -- ETSY
);

-- → Food > Restaurants (f5b416b6) — miscategorized as Shopping
UPDATE transactions SET category_id = 'f5b416b6-1c32-427d-b6f3-ec551bd6f872'
WHERE id IN (
  '682b96a2-7997-41ab-9b78-685359c1932d',  -- Dong Kinh (Vietnamese restaurant)
  '1a29880d-a0b3-4d2a-b34a-09e3e6ac6ce9',  -- Sbehrenbachgrabenhütte (ski hut)
  'dd70c6e8-cb9f-4548-bd90-2629c3542251',  -- BK (Burger King)
  '87c1978c-8448-4bd8-b594-4d2abd4dbe99'   -- BK (Burger King)
);

-- → Fees > Bank Fees (1185e77c) — Zulassungsstelle (vehicle registration)
UPDATE transactions SET category_id = '1185e77c-3e19-42c2-b335-cf62a4fb0254'
WHERE id = 'c3ec2e72-64c5-4765-91ac-3e7ee8712b63';

-- → Entertainment > Holidays (25fd2c57) — travel purchases
UPDATE transactions SET category_id = '25fd2c57-2ac4-4828-8aef-3d76e42ff337'
WHERE id IN (
  '9260551f-146c-407e-9294-d7b8203a6d23',  -- Benedict d.o.o. (Ljubljana)
  'a2237427-e2aa-46f8-86f8-069695f14d40',  -- Hewitt LG (Australia)
  '7effa6b2-c563-4348-818d-f72bf4cf6a90',  -- Hewitt LG (Australia)
  'd6c0587a-619b-435e-b2a0-11bb0bf8a831',  -- Susso and Ng (Australia)
  'f8e4173b-fae5-4ff5-92b5-177d1de9ac87'   -- Kaufhaus Lutz (Austria)
);

-- → Family > Donations — Wildtierhilfe BW
UPDATE transactions SET category_id = 'a0000001-0000-0000-0000-000000000007'
WHERE id = 'a8625ae3-6967-49fe-9d44-3002d0dbb6a6';

-- Transportation (parent: 38417a4e) → children
-- Kienle Transport → House > Renovation (fbbb1b3f)
UPDATE transactions SET category_id = 'fbbb1b3f-df0b-496a-9f90-f31b852395ac'
WHERE id IN (
  '8c59be96-3c8d-4e11-881a-ee375da6ca0b',
  '4a16a1b2-4d88-44a1-a80c-e3479fa03f60'
);

-- VW Leasing → Transportation > Audi (045c3c3a)
UPDATE transactions SET category_id = '045c3c3a-4793-4870-ad40-480b9725f829'
WHERE id IN (
  '120d60c9-2036-4f1d-936a-20af1971db74',
  '16d29f97-fda5-4c4c-9c27-a99ef0492b07'
);

-- ============================================================
-- 4. Kienle Transport auto-categorization rule
-- ============================================================

INSERT INTO rules (id, rule_type, conditions, target_category_id, priority)
VALUES (
  'a0000001-0000-0000-0000-000000000010',
  'categorization',
  '[{"field": "merchant", "pattern": "Kienle Transport"}]'::jsonb,
  'fbbb1b3f-df0b-496a-9f90-f31b852395ac',
  0
);
