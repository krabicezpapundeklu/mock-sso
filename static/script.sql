INSERT INTO sso_idp_pk_repository (idp_id, idp_name, idp_desc, idp_pk_data, issuer_url, fedr_auth_attr, mgs_auth_attr,
                                   is_case_sensitive)
SELECT (SELECT NVL(MAX(idp_id), 0) + 1 FROM sso_idp_pk_repository),
       'MOCK-SSO',
       'This is IDP for SAML authentications against MOCK-SSO',
       '-----BEGIN CERTIFICATE-----
      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
      -----END CERTIFICATE-----',
       'https://mock-sso.mgspdtesting.com',
       'NameID',
       'u.userid',
       0
FROM dual
WHERE NOT EXISTS (SELECT 1 FROM sso_idp_pk_repository WHERE idp_name = 'MOCK-SSO');
