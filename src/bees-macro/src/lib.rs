use proc_macro::TokenStream as NativeTokenStream;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};
use syn::spanned::Spanned;

#[proc_macro_derive(Struct)]
pub fn derive_struct(input: NativeTokenStream) -> NativeTokenStream {
    let crate_ = quote!(::bees);
    let crate_internal = quote!(#crate_::derive_struct_internal);
    let input: syn::DeriveInput = syn::parse_macro_input!(input);

    // Generate names
    let vis = input.vis;
    let wrapped_name = input.ident.to_string();
    let wrapper_name = format!("{}Ref", wrapped_name);

    let base_name = Ident::new(&wrapped_name, input.ident.span());
    let wrapper_name = Ident::new(&wrapper_name, input.ident.span());

    // Generate generic signatures
    let generic_in_list = input.generics.params.iter().collect::<Vec<_>>();
    let where_clause = match &input.generics.where_clause {
        Some(clause) => clause.to_token_stream(),
        None => TokenStream::new(),
    };

    let generic_fwd_list = input
        .generics
        .params
        .iter()
        .map(|para| match para {
            syn::GenericParam::Lifetime(lt) => lt.lifetime.to_token_stream(),
            syn::GenericParam::Type(ty) => ty.ident.to_token_stream(),
            syn::GenericParam::Const(cst) => cst.ident.to_token_stream(),
        })
        .collect::<Vec<_>>();

    // Generate field accessors
    let fields = match input.data {
        syn::Data::Struct(stt) => stt.fields,
        syn::Data::Enum(enn) => {
            return syn::Error::new(enn.enum_token.span(), "Struct cannot be derived on enums.")
                .into_compile_error()
                .into();
        }
        syn::Data::Union(unn) => {
            return syn::Error::new(
                unn.union_token.span(),
                "Struct cannot be derived on unions.",
            )
            .into_compile_error()
            .into();
        }
    };

    let fields = match &fields {
        syn::Fields::Named(fields) => fields
            .named
            .iter()
            .map(|field| {
                (
                    field.ident.clone().unwrap(),
                    field.ident.clone().unwrap(),
                    field,
                )
            })
            .collect(),
        syn::Fields::Unnamed(fields) => fields
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, field)| {
                (
                    Ident::new(&i.to_string(), Span::call_site()),
                    Ident::new(&format!("tup_{i}"), Span::call_site()),
                    field,
                )
            })
            .collect(),
        syn::Fields::Unit => Vec::new(),
    };

    let accessors = fields
        .iter()
        .map(|(field_name, method_name_base, field)| {
            let vis = &field.vis;
            let ty = &field.ty;

            let method_name_get = method_name_base.clone();
            let method_name_set =
                Ident::new(&format!("set_{method_name_base}"), method_name_base.span());

            let method_name_prim_ref =
                Ident::new(&format!("{method_name_base}_prim_ref"), method_name_base.span());

			let method_name_ref =
                Ident::new(&format!("{method_name_base}_ref"), method_name_base.span());

            quote! {
                #vis fn #method_name_prim_ref(&self) -> #crate_::WideRef<#ty>
                where
                    for<'__trivial> <#ty as #crate_internal::TrivialBound<'__trivial>>::Itself: Sized,
                {
                    #crate_::subfield!(self.0, #field_name)
                }

				#vis fn #method_name_ref<__WideRefOut>(&self) -> __WideRefOut
                where
                    for<'__trivial> <#ty as #crate_internal::TrivialBound<'__trivial>>::Itself: Sized + #crate_::Struct<WideWrapper = __WideRefOut>,
					__WideRefOut: #crate_::WideWrapper<Pointee = #ty>,
                {
                    #crate_::WideWrapper::from_raw(self.#method_name_prim_ref())
                }

                #vis fn #method_name_get(&self) -> #ty
                where
					for<'__trivial> <#ty as #crate_internal::TrivialBound<'__trivial>>::Itself: #crate_internal::Copy,
                {
                    self.#method_name_prim_ref().read()
                }

                #vis fn #method_name_set(&self, value: #ty)
				where
                    for<'__trivial> <#ty as #crate_internal::TrivialBound<'__trivial>>::Itself: Sized,
				{
                    self.#method_name_prim_ref().write(value);
                }
            }
        })
        .collect::<Vec<_>>();

    let output = quote! {
        #vis struct #wrapper_name<#(#generic_in_list),*>(#crate_::WideRef<#base_name<#(#generic_fwd_list),*>>)
        #where_clause;

        impl<#(#generic_in_list),*> #crate_internal::Copy for #wrapper_name<#(#generic_fwd_list),*>
        #where_clause
        {}

        impl<#(#generic_in_list),*> #crate_internal::Clone for #wrapper_name<#(#generic_fwd_list),*>
        #where_clause
        {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<#(#generic_in_list),*> #crate_::Struct for #base_name<#(#generic_fwd_list),*>
        #where_clause
        {
            type WideWrapper = #wrapper_name<#(#generic_fwd_list),*>;
        }

        impl<#(#generic_in_list),*> #crate_::WideWrapper for #wrapper_name<#(#generic_fwd_list),*>
        #where_clause
        {
            type Pointee = #base_name<#(#generic_fwd_list),*>;

            fn from_raw(raw: #crate_::WideRef<Self::Pointee>) -> Self {
                Self(raw)
            }

            fn raw(self) -> #crate_::WideRef<Self::Pointee> {
                self.0
            }
        }

        impl<#(#generic_in_list),*> #wrapper_name<#(#generic_fwd_list),*>
        #where_clause
        {
            #(#accessors)*
        }
    };

    output.into()
}
